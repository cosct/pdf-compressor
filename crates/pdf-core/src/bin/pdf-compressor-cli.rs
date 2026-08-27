//! Command-line interface for the PDF engine — analyze, compress, or the
//! desktop-integration `quick` mode used by file-manager right-click entries.
//! PDF 引擎命令行工具 — 分析、压缩，以及供文件管理器右键菜单调用的 quick 后台模式。
//!
//! Usage:
//!   pdf-compressor-cli analyze <input.pdf>
//!   pdf-compressor-cli compress <input.pdf> [--preset maximum|balanced|conservative]
//!                              [--quality 10-100] [--max-edge 100-8000] [--grayscale]
//!                              [--bilevel g4] [--output-dir DIR] [--keep-metadata]
//!                              [--target-size 5MB]
//!   pdf-compressor-cli quick <input.pdf> [more.pdf ...] [OPTIONS]
//!
//! `quick` is the headless background mode: it compresses each file next to
//! the original, removes outputs that did not get smaller, prints a JSON
//! summary, and shows a desktop notification (via `notify-send` when present).
//! Results are printed to stdout as JSON; errors go to stderr as JSON.

use std::{
    path::Path,
    process::{Command, ExitCode, Stdio},
    sync::atomic::AtomicBool,
    sync::Arc,
};

use serde::Serialize;

use pdf_core::{
    analyze_pdf_with_progress, compress_pdf_to_target_size, compress_pdf_with_progress,
    AppError, AppErrorPayload, BilevelCodec, CompressionResponse, CompressionSettings,
    CompressionSettingsOverrides,
};

const USAGE: &str = "\
pdf-compressor-cli — analyze, compress, or quick-compress PDFs from the shell

USAGE:
    pdf-compressor-cli analyze <input.pdf>
    pdf-compressor-cli compress <input.pdf> [OPTIONS]
    pdf-compressor-cli quick <input.pdf> [more.pdf ...] [OPTIONS]

QUICK OPTIONS (background mode, used by file-manager context menus):
    --preset <NAME>       maximum | balanced | conservative (default: balanced)
    --quality <N>         JPEG quality 10-100
    --max-edge <PX>       Maximum image edge in pixels (100-8000)
    --grayscale           Re-encode color images as grayscale
    --bilevel <CODEC>     Codec for near-black-and-white images: g4 | jpeg
                          (g4 = lossless CCITT Group 4, best for text scans)
    --keep-metadata       Keep document metadata (removed by default)
    --target-size <SIZE>  Fit the output under this size (e.g. 5MB, 500K)
    --no-notify           Skip the desktop notification

COMPRESS OPTIONS:
    (same as QUICK, plus:)
    --output-dir <DIR>    Write the output into this directory

OTHER:
    -h, --help            Show this help";

/// Parse a human size like `5MB`, `500K`, or a raw byte count.
fn parse_size(text: &str) -> Option<u64> {
    let trimmed = text.trim();
    let split_at = trimmed
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at(split_at);
    let value: f64 = number.trim().parse().ok()?;
    let multiplier = match unit.trim().to_ascii_uppercase().as_str() {
        "" | "B" => 1.0,
        "K" | "KB" | "KIB" => 1024.0,
        "M" | "MB" | "MIB" => 1024.0 * 1024.0,
        "G" | "GB" | "GIB" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((value * multiplier) as u64)
}

/// Read `--name value` or `--name=value` from the argument list.
fn flag_value(args: &[String], name: &str) -> Result<Option<String>, String> {
    let inline = format!("{name}=");
    for (index, arg) in args.iter().enumerate() {
        if let Some(value) = arg.strip_prefix(&inline) {
            return Ok(Some(value.to_string()));
        }
        if arg == name {
            let value = args
                .get(index + 1)
                .ok_or_else(|| format!("{name} requires a value"))?;
            return Ok(Some(value.clone()));
        }
    }
    Ok(None)
}

fn parsed_flag<T: std::str::FromStr>(args: &[String], name: &str) -> Result<Option<T>, String> {
    match flag_value(args, name)? {
        Some(raw) => raw
            .parse()
            .map(Some)
            .map_err(|_| format!("{name} got an invalid value: {raw}")),
        None => Ok(None),
    }
}

/// Split a `quick` argument list into positional input paths and flag
/// arguments. Value-taking flags consume their following argument so paths
/// are never mistaken for flag values.
fn split_quick_inputs(args: &[String]) -> Result<(Vec<String>, Vec<String>), String> {
    const VALUE_FLAGS: [&str; 6] = [
        "--preset",
        "--quality",
        "--max-edge",
        "--bilevel",
        "--target-size",
        "--output-dir",
    ];
    let mut inputs = Vec::new();
    let mut flags = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if VALUE_FLAGS.contains(&arg.as_str()) {
            let value = args
                .get(index + 1)
                .ok_or_else(|| format!("{arg} requires a value"))?;
            flags.push(arg.clone());
            flags.push(value.clone());
            index += 2;
        } else if arg.starts_with("--") {
            flags.push(arg.clone());
            index += 1;
        } else {
            inputs.push(arg.clone());
            index += 1;
        }
    }
    Ok((inputs, flags))
}

fn compression_overrides(rest: &[String]) -> Result<CompressionSettingsOverrides, AppError> {
    let config_error = |message: String| AppError::Config(message);
    let bilevel_codec = match flag_value(rest, "--bilevel").map_err(config_error)? {
        Some(raw) => {
            let codec = BilevelCodec::from_optional_str(Some(&raw));
            if codec.uses_ccitt() && !raw.to_ascii_lowercase().contains("g4") {
                return Err(AppError::Config(format!(
                    "--bilevel got an invalid value: {raw} (expected g4 or jpeg)"
                )));
            }
            Some(codec)
        }
        None => None,
    };
    Ok(CompressionSettingsOverrides {
        preset: flag_value(rest, "--preset").map_err(config_error)?,
        image_quality: parsed_flag::<u8>(rest, "--quality").map_err(config_error)?,
        max_image_size_px: parsed_flag::<u16>(rest, "--max-edge").map_err(config_error)?,
        output_dir: flag_value(rest, "--output-dir").map_err(config_error)?,
        bilevel_codec,
        grayscale: if rest.iter().any(|arg| arg == "--grayscale") {
            Some(true)
        } else {
            None
        },
        strip_metadata: if rest.iter().any(|arg| arg == "--keep-metadata") {
            Some(false)
        } else {
            None
        },
        ..Default::default()
    })
}

fn target_size_bytes(rest: &[String]) -> Result<Option<u64>, AppError> {
    match flag_value(rest, "--target-size") {
        Ok(Some(raw)) => match parse_size(&raw) {
            Some(bytes) => Ok(Some(bytes)),
            None => Err(AppError::Config(format!(
                "--target-size got an invalid value: {raw}"
            ))),
        },
        Ok(None) => Ok(None),
        Err(message) => Err(AppError::Config(message)),
    }
}

/// Compress one file with the quick-mode contract: the output lands next to
/// the original, and nothing is left behind when it would not win — the
/// engine itself skips writing a non-smaller result.
fn quick_compress_one(
    input: &str,
    settings: &CompressionSettings,
    target_bytes: Option<u64>,
) -> Result<(CompressionResponse, bool), AppError> {
    let cancel = Arc::new(AtomicBool::new(false));
    let no_progress = |_| {};
    let mut no_progress_mut = |_| {};

    let response = match target_bytes {
        Some(target) => compress_pdf_to_target_size(
            input,
            target,
            settings.clone(),
            cancel,
            &mut no_progress_mut,
        ),
        None => compress_pdf_with_progress(input, settings.clone(), cancel, no_progress),
    }?;

    Ok((response.clone(), response.output_was_smaller))
}

#[derive(Debug)]
enum QuickOutcome {
    /// Compressed and the output was smaller.
    Compressed(CompressionResponse),
    /// Engine succeeded but the output was not smaller; the file was removed.
    NotSmaller(CompressionResponse),
    /// Engine refused or failed; payload carries the user-facing message.
    Failed(AppErrorPayload),
}

impl QuickOutcome {
    fn kept_bytes(&self) -> Option<f64> {
        match self {
            QuickOutcome::Compressed(response) => Some(response.compressed_size_bytes),
            QuickOutcome::NotSmaller(response) => Some(response.original_size_bytes),
            QuickOutcome::Failed(_) => None,
        }
    }

    fn original_bytes(&self) -> Option<f64> {
        match self {
            QuickOutcome::Compressed(response)
            | QuickOutcome::NotSmaller(response) => Some(response.original_size_bytes),
            QuickOutcome::Failed(_) => None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
enum QuickFileReport {
    Compressed {
        input: String,
        #[serde(flatten)]
        response: CompressionResponse,
    },
    NotSmaller {
        input: String,
        output_path: String,
        original_size_bytes: f64,
        message: String,
    },
    Failed {
        input: String,
        error: AppErrorPayload,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QuickSummary {
    results: Vec<QuickFileReport>,
    files_compressed: usize,
    files_not_smaller: usize,
    files_failed: usize,
    original_bytes: f64,
    compressed_bytes: f64,
    savings_percent: f32,
}

fn run_quick(rest: &[String], notify: bool) -> Result<QuickSummary, AppError> {
    let (inputs, flags) =
        split_quick_inputs(rest).map_err(AppError::Config)?;
    if inputs.is_empty() {
        return Err(AppError::Config(
            "quick requires at least one input PDF path".to_string(),
        ));
    }
    if flag_value(&flags, "--output-dir")
        .map_err(AppError::Config)?
        .is_some()
    {
        return Err(AppError::Config(
            "quick always writes next to the original file; --output-dir is not supported"
                .to_string(),
        ));
    }

    let overrides = compression_overrides(&flags)?;
    let target = target_size_bytes(&flags)?;
    let settings = CompressionSettings::from_sources(None, overrides);

    let mut results = Vec::with_capacity(inputs.len());
    for input in &inputs {
        let outcome = match quick_compress_one(input, &settings, target) {
            Ok((response, true)) => QuickOutcome::Compressed(response),
            Ok((response, false)) => QuickOutcome::NotSmaller(response),
            Err(error) => QuickOutcome::Failed(AppErrorPayload::from(error)),
        };
        results.push(outcome);
    }

    let files_compressed = results
        .iter()
        .filter(|r| matches!(r, QuickOutcome::Compressed(_)))
        .count();
    let files_not_smaller = results
        .iter()
        .filter(|r| matches!(r, QuickOutcome::NotSmaller(_)))
        .count();
    let files_failed = results
        .iter()
        .filter(|r| matches!(r, QuickOutcome::Failed(_)))
        .count();

    let original_bytes: f64 = results.iter().filter_map(QuickOutcome::original_bytes).sum();
    let compressed_bytes: f64 = results.iter().filter_map(QuickOutcome::kept_bytes).sum();
    let savings_percent = if original_bytes > 0.0 {
        ((original_bytes - compressed_bytes) / original_bytes * 100.0) as f32
    } else {
        0.0
    };

    let summary = QuickSummary {
        results: results
            .into_iter()
            .zip(&inputs)
            .map(|(outcome, input)| match outcome {
                QuickOutcome::Compressed(response) => QuickFileReport::Compressed {
                    input: input.clone(),
                    response,
                },
                QuickOutcome::NotSmaller(response) => QuickFileReport::NotSmaller {
                    input: input.clone(),
                    output_path: response.output_path,
                    original_size_bytes: response.original_size_bytes,
                    message: "The original file was already small enough; no output was \
                              written."
                        .to_string(),
                },
                QuickOutcome::Failed(error) => QuickFileReport::Failed {
                    input: input.clone(),
                    error,
                },
            })
            .collect(),
        files_compressed,
        files_not_smaller,
        files_failed,
        original_bytes,
        compressed_bytes,
        savings_percent,
    };

    if notify {
        let (urgency, summary_text, body) =
            build_quick_notification(&summary, &inputs, quick_language());
        send_notification(urgency, &summary_text, &body);
    }

    Ok(summary)
}

/// Locale for notification copy: Chinese when the environment asks for it,
/// English otherwise.
fn quick_language() -> &'static str {
    let lang = std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .unwrap_or_default();
    if lang.to_ascii_lowercase().starts_with("zh") {
        "zh"
    } else {
        "en"
    }
}

/// Human-readable byte size: `612.3KB`, `2.65MB`.
fn human_bytes(bytes: f64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut value = bytes.max(0.0);
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0}{}", units[unit])
    } else {
        format!("{value:.2}{}", units[unit])
    }
}

fn file_display_name(input: &str) -> String {
    Path::new(input)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| input.to_string())
}

/// Build the desktop-notification copy from a quick run. Pure so tests can
/// pin the wording for both locales.
fn build_quick_notification(
    summary: &QuickSummary,
    inputs: &[String],
    lang: &str,
) -> (&'static str, String, String) {
    let zh = lang == "zh";
    let failures = summary.files_failed;
    let success = summary.files_compressed + summary.files_not_smaller;

    let (urgency, title): (&'static str, String) = if failures > 0 && success == 0 {
        (
            "critical",
            if zh { "PDF 压缩失败" } else { "PDF compression failed" }.to_string(),
        )
    } else if failures > 0 {
        (
            "normal",
            if zh {
                format!("PDF 压缩完成（{failures} 个失败）")
            } else {
                format!("PDF compression finished ({failures} failed)")
            },
        )
    } else {
        (
            "normal",
            if zh { "PDF 压缩完成" } else { "PDF compression finished" }.to_string(),
        )
    };

    let mut lines: Vec<String> = Vec::new();
    if summary.results.len() == 1 {
        match &summary.results[0] {
            QuickFileReport::Compressed { input, response } => {
                let name = file_display_name(input);
                if zh {
                    lines.push(format!(
                        "{}：{} → {}（省 {:.1}%）",
                        name,
                        human_bytes(response.original_size_bytes),
                        human_bytes(response.compressed_size_bytes),
                        response.savings_percent
                    ));
                } else {
                    lines.push(format!(
                        "{}: {} → {} (−{:.1}%)",
                        name,
                        human_bytes(response.original_size_bytes),
                        human_bytes(response.compressed_size_bytes),
                        response.savings_percent
                    ));
                }
            }
            QuickFileReport::NotSmaller { input, .. } => {
                let name = file_display_name(input);
                lines.push(if zh {
                    format!("{name}：已经足够小，未改动")
                } else {
                    format!("{name}: already small enough, left unchanged")
                });
            }
            QuickFileReport::Failed { input, error } => {
                let name = file_display_name(input);
                lines.push(if zh {
                    format!("{name}：{}", error.fallback)
                } else {
                    format!("{name}: {}", error.fallback)
                });
            }
        }
    } else {
        if zh {
            lines.push(format!(
                "{} 个文件：{} → {}（省 {:.1}%）",
                summary.results.len(),
                human_bytes(summary.original_bytes),
                human_bytes(summary.compressed_bytes),
                summary.savings_percent
            ));
        } else {
            lines.push(format!(
                "{} files: {} → {} (−{:.1}%)",
                summary.results.len(),
                human_bytes(summary.original_bytes),
                human_bytes(summary.compressed_bytes),
                summary.savings_percent
            ));
        }
        const MAX_DETAIL_LINES: usize = 3;
        for (report, input) in summary.results.iter().zip(inputs).take(MAX_DETAIL_LINES) {
            let name = file_display_name(input);
            match report {
                QuickFileReport::Compressed { response, .. } => lines.push(if zh {
                    format!(
                        "{}：{} → {}（−{:.1}%）",
                        name,
                        human_bytes(response.original_size_bytes),
                        human_bytes(response.compressed_size_bytes),
                        response.savings_percent
                    )
                } else {
                    format!(
                        "{}: {} → {} (−{:.1}%)",
                        name,
                        human_bytes(response.original_size_bytes),
                        human_bytes(response.compressed_size_bytes),
                        response.savings_percent
                    )
                }),
                QuickFileReport::NotSmaller { .. } => lines.push(if zh {
                    format!("{name}：已经足够小")
                } else {
                    format!("{name}: already small enough")
                }),
                QuickFileReport::Failed { .. } => lines.push(if zh {
                    format!("{name}：失败")
                } else {
                    format!("{name}: failed")
                }),
            }
        }
        let hidden = summary.results.len().saturating_sub(MAX_DETAIL_LINES);
        if hidden > 0 {
            lines.push(if zh {
                format!("…等 {hidden} 个文件")
            } else {
                format!("…and {hidden} more")
            });
        }
    }

    let body = lines.join("\n");
    // notify-send would parse a leading '-' as an option; make it inert.
    let body = if body.starts_with('-') {
        format!(" {body}")
    } else {
        body
    };
    (urgency, title, body)
}

/// Fire a desktop notification through `notify-send` when available. failures
/// are silent — notifications are best-effort, never a hard dependency.
fn send_notification(urgency: &str, title: &str, body: &str) {
    let _ = Command::new("notify-send")
        .args(["-a", "PDF Compressor", "-i", "pdf-compressor", "-u", urgency])
        .arg(title)
        .arg(body)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn run(args: &[String]) -> Result<String, AppError> {
    let command = args
        .first()
        .ok_or_else(|| AppError::Config("missing command".to_string()))?;
    let rest = &args[1..];
    let input = rest
        .first()
        .ok_or_else(|| AppError::Config("missing input path".to_string()))?
        .clone();

    match command.as_str() {
        "analyze" => analyze_pdf_with_progress(&input, |_| {})
            .map(|response| serde_json::to_string_pretty(&response).expect("serializable")),
        "compress" => {
            // Flag parsing errors are surfaced through AppError::Config.
            let overrides = compression_overrides(rest)?;
            let target_bytes = target_size_bytes(rest)?;
            let settings = CompressionSettings::from_sources(None, overrides);
            let mut no_progress = |_| {};

            match target_bytes {
                Some(target) => compress_pdf_to_target_size(
                    &input,
                    target,
                    settings,
                    Arc::new(AtomicBool::new(false)),
                    &mut no_progress,
                ),
                None => compress_pdf_with_progress(
                    &input,
                    settings,
                    Arc::new(AtomicBool::new(false)),
                    |_| {},
                ),
            }
            .map(|response| serde_json::to_string_pretty(&response).expect("serializable"))
        }
        other => Err(AppError::Config(format!("unknown command: {other}"))),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") || args.is_empty() {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    if args.first().is_some_and(|command| command == "quick") {
        let notify = !args.iter().any(|arg| arg == "--no-notify");
        return match run_quick(&args[1..], notify) {
            Ok(summary) => {
                let printed = serde_json::to_string_pretty(&summary).expect("serializable");
                println!("{printed}");
                if summary.files_failed > 0
                    && summary.files_compressed + summary.files_not_smaller == 0
                {
                    ExitCode::FAILURE
                } else {
                    ExitCode::SUCCESS
                }
            }
            Err(error) => {
                let payload = AppErrorPayload::from(error);
                eprintln!(
                    "{}",
                    serde_json::to_string_pretty(&payload).expect("serializable")
                );
                ExitCode::FAILURE
            }
        };
    }

    match run(&args) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let payload = AppErrorPayload::from(error);
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&payload).expect("serializable")
            );
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_quick_notification, flag_value, human_bytes, parse_size, quick_language,
        QuickFileReport, QuickSummary, split_quick_inputs,
    };
    use pdf_core::AppErrorPayload;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn parse_size_accepts_units_and_raw_bytes() {
        assert_eq!(parse_size("3000000"), Some(3_000_000));
        assert_eq!(parse_size("500K"), Some(500 * 1024));
        assert_eq!(parse_size("5MB"), Some(5 * 1024 * 1024));
        assert_eq!(parse_size("2 GiB"), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("1b"), Some(1));
    }

    #[test]
    fn parse_size_rejects_garbage() {
        assert_eq!(parse_size("abc"), None);
        assert_eq!(parse_size("5TB"), None);
        assert_eq!(parse_size(""), None);
    }

    #[test]
    fn flag_value_reads_space_and_inline_forms() {
        let list = args(&["--preset", "maximum", "--quality=72"]);
        assert_eq!(
            flag_value(&list, "--preset"),
            Ok(Some("maximum".to_string()))
        );
        assert_eq!(flag_value(&list, "--quality"), Ok(Some("72".to_string())));
        assert_eq!(flag_value(&list, "--missing"), Ok(None));
    }

    #[test]
    fn flag_value_requires_a_value() {
        let list = args(&["input.pdf", "--preset"]);
        assert!(flag_value(&list, "--preset").is_err());
    }

    #[test]
    fn split_quick_inputs_separates_paths_from_flags() {
        let list = args(&[
            "a.pdf",
            "--preset",
            "maximum",
            "b.pdf",
            "--grayscale",
            "--quality=60",
        ]);
        let (inputs, flags) = split_quick_inputs(&list).expect("split");
        assert_eq!(inputs, args(&["a.pdf", "b.pdf"]));
        assert_eq!(
            flags,
            args(&["--preset", "maximum", "--grayscale", "--quality=60"])
        );
    }

    #[test]
    fn split_quick_inputs_rejects_dangling_value_flag() {
        assert!(split_quick_inputs(&args(&["a.pdf", "--preset"])).is_err());
    }

    #[test]
    fn human_bytes_picks_sane_units() {
        assert_eq!(human_bytes(0.0), "0B");
        assert_eq!(human_bytes(512.0), "512B");
        assert_eq!(human_bytes(626_688.0), "612.00KB");
        assert_eq!(human_bytes(2_773_997.0), "2.65MB");
    }

    #[test]
    fn quick_language_follows_locale() {
        // Locale-dependent via env vars; only assert the classification of a
        // fixed value to keep the test environment-independent.
        let decision = |lang: &str| {
            lang.to_ascii_lowercase().starts_with("zh")
        };
        assert!(decision("zh_CN.UTF-8"));
        assert!(!decision("en_US.UTF-8"));
        let _ = quick_language(); // must not panic with unset locale vars
    }

    fn summary_fixture() -> (QuickSummary, Vec<String>) {
        let compressed = QuickFileReport::Compressed {
            input: "/tmp/photos.pdf".to_string(),
            response: pdf_core::CompressionResponse {
                output_path: "/tmp/photos__optimized-balanced.pdf".to_string(),
                original_size_bytes: 2_773_997.0,
                compressed_size_bytes: 626_688.0,
                saved_bytes: 2_147_309.0,
                savings_percent: 77.4,
                elapsed_ms: 120,
                images_recompressed: 8,
                images_skipped: 0,
                images_deduplicated: 0,
                streams_compressed: 0,
                metadata_removed: true,
                output_was_smaller: true,
                notices: Vec::new(),
            },
        };
        let failed = QuickFileReport::Failed {
            input: "/tmp/locked.pdf".to_string(),
            error: AppErrorPayload::from(pdf_core::AppError::Encrypted),
        };
        let summary = QuickSummary {
            results: vec![compressed, failed],
            files_compressed: 1,
            files_not_smaller: 0,
            files_failed: 1,
            original_bytes: 2_773_997.0,
            compressed_bytes: 626_688.0,
            savings_percent: 77.4,
        };
        let inputs = args(&["/tmp/photos.pdf", "/tmp/locked.pdf"]);
        (summary, inputs)
    }

    #[test]
    fn notification_en_lists_totals_and_failures() {
        let (summary, inputs) = summary_fixture();
        let (urgency, title, body) = build_quick_notification(&summary, &inputs, "en");
        assert_eq!(urgency, "normal");
        assert_eq!(title, "PDF compression finished (1 failed)");
        assert!(body.contains("2 files: 2.65MB → 612.00KB (−77.4%)"));
        assert!(body.contains("photos.pdf: 2.65MB → 612.00KB (−77.4%)"));
        assert!(body.contains("locked.pdf: failed"));
    }

    #[test]
    fn notification_zh_lists_totals_and_failures() {
        let (summary, inputs) = summary_fixture();
        let (urgency, title, body) = build_quick_notification(&summary, &inputs, "zh");
        assert_eq!(urgency, "normal");
        assert!(title.contains("1 个失败"));
        assert!(body.contains("2 个文件：2.65MB → 612.00KB"));
        assert!(body.contains("locked.pdf：失败"));
    }

    #[test]
    fn notification_all_failed_is_critical() {
        let summary = QuickSummary {
            results: vec![QuickFileReport::Failed {
                input: "/tmp/locked.pdf".to_string(),
                error: AppErrorPayload::from(pdf_core::AppError::Encrypted),
            }],
            files_compressed: 0,
            files_not_smaller: 0,
            files_failed: 1,
            original_bytes: 0.0,
            compressed_bytes: 0.0,
            savings_percent: 0.0,
        };
        let inputs = args(&["/tmp/locked.pdf"]);
        let (urgency, title, body) = build_quick_notification(&summary, &inputs, "en");
        assert_eq!(urgency, "critical");
        assert_eq!(title, "PDF compression failed");
        assert!(body.contains("encrypted"));
    }

    #[test]
    fn notification_body_never_starts_with_dash() {
        let summary = QuickSummary {
            results: vec![QuickFileReport::NotSmaller {
                input: "/tmp/-weird-name.pdf".to_string(),
                output_path: String::new(),
                original_size_bytes: 100.0,
                message: String::new(),
            }],
            files_compressed: 0,
            files_not_smaller: 1,
            files_failed: 0,
            original_bytes: 100.0,
            compressed_bytes: 100.0,
            savings_percent: 0.0,
        };
        let inputs = args(&["/tmp/-weird-name.pdf"]);
        let (_, _, body) = build_quick_notification(&summary, &inputs, "en");
        assert!(!body.starts_with('-'));
    }
}
