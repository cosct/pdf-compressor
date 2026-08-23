//! Minimal CLI for the PDF engine — analyze or compress a file from the shell.
//! PDF 引擎的最小命令行工具 — 在终端分析或压缩文件。
//!
//! Usage:
//!   pdf-cli analyze <input.pdf>
//!   pdf-cli compress <input.pdf> [--preset maximum|balanced|conservative]
//!                    [--quality 10-100] [--max-edge 100-8000]
//!                    [--output-dir DIR] [--keep-metadata]
//!                    [--target-size 5MB]
//!
//! Results are printed to stdout as JSON; errors go to stderr as JSON.

use std::{process::ExitCode, sync::atomic::AtomicBool, sync::Arc};

use pdf_core::{
    analyze_pdf_with_progress, compress_pdf_to_target_size, compress_pdf_with_progress,
    AppError, AppErrorPayload, CompressionSettings, CompressionSettingsOverrides,
};

const USAGE: &str = "\
pdf-cli — analyze or compress a PDF from the shell

USAGE:
    pdf-cli analyze <input.pdf>
    pdf-cli compress <input.pdf> [OPTIONS]

OPTIONS:
    --preset <NAME>       maximum | balanced | conservative (default: balanced)
    --quality <N>         JPEG quality 10-100
    --max-edge <PX>       Maximum image edge in pixels (100-8000)
    --output-dir <DIR>    Write the output into this directory
    --keep-metadata       Keep document metadata (removed by default)
    --target-size <SIZE>  Fit the output under this size (e.g. 5MB, 500K, 3000000)
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
            let target_bytes = match flag_value(rest, "--target-size") {
                Ok(Some(raw)) => match parse_size(&raw) {
                    Some(bytes) => Some(bytes),
                    None => {
                        return Err(AppError::Config(format!(
                            "--target-size got an invalid value: {raw}"
                        )))
                    }
                },
                Ok(None) => None,
                Err(message) => return Err(AppError::Config(message)),
            };
            let settings = CompressionSettings::from_sources(None, overrides);
            let mut no_progress = |_| {};

            match target_bytes {
                Some(target) => {
                    compress_pdf_to_target_size(&input, target, settings, Arc::new(AtomicBool::new(false)), &mut no_progress)
                }
                None => compress_pdf_with_progress(
                    &input,
                    settings,
                    Arc::new(AtomicBool::new(false)),
                    |_| {},
                ),
            }
            .map(|response| {
                serde_json::to_string_pretty(&response).expect("serializable")
            })
        }
        other => Err(AppError::Config(format!("unknown command: {other}"))),
    }
}

fn compression_overrides(rest: &[String]) -> Result<CompressionSettingsOverrides, AppError> {
    let config_error = |message: String| AppError::Config(message);
    Ok(CompressionSettingsOverrides {
        preset: flag_value(rest, "--preset").map_err(config_error)?,
        image_quality: parsed_flag::<u8>(rest, "--quality").map_err(config_error)?,
        max_image_size_px: parsed_flag::<u16>(rest, "--max-edge").map_err(config_error)?,
        output_dir: flag_value(rest, "--output-dir").map_err(config_error)?,
        strip_metadata: if rest.iter().any(|arg| arg == "--keep-metadata") {
            Some(false)
        } else {
            None
        },
        ..Default::default()
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") || args.is_empty() {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
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
