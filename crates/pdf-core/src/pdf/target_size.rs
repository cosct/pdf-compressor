//! Target-size compression — search quality/edge parameters until the output
//! fits a byte budget, or return the best effort.
//! 目标大小压缩 — 搜索质量/边长参数直到输出满足字节预算，否则尽力而为。

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::{
    error::AppError,
    models::{BackendNotice, CompressionResponse, ProgressUpdate},
};

use super::compressor::compress_pdf_with_progress;
use super::settings::CompressionSettings;

/// Lower bound of the quality binary search. Below this, JPEG artifacts
/// destroy the document; the edge shrink takes over instead.
const MIN_SEARCH_QUALITY: u8 = 15;

/// Upper bound on probe attempts before settling for the best effort.
const MAX_ATTEMPTS: usize = 6;

/// Never shrink the max edge below this while fighting for a budget.
const MIN_SEARCH_EDGE: u16 = 400;

/// Compress `path` so the output fits within `target_bytes` when possible.
///
/// The search runs full pipeline probes into a scratch directory (binary
/// search on JPEG quality, shrinking the max edge on late attempts), then a
/// final pass writes the winning parameters next to the source file. When the
/// budget cannot be met, the smallest achievable result is produced with a
/// warning notice.
pub fn compress_pdf_to_target_size<F>(
    path: &str,
    target_bytes: u64,
    settings: CompressionSettings,
    cancel_flag: Arc<AtomicBool>,
    report_progress: &mut F,
) -> Result<CompressionResponse, AppError>
where
    F: FnMut(ProgressUpdate),
{
    if target_bytes == 0 {
        return Err(AppError::Config(
            "Target size must be greater than zero bytes.".to_string(),
        ));
    }

    let scratch = tempfile::tempdir().map_err(AppError::Io)?;
    let scratch_dir = scratch.path().to_string_lossy().into_owned();

    fn probe<F>(
        path: &str,
        quality: u8,
        edge: u16,
        settings: &CompressionSettings,
        cancel_flag: &Arc<AtomicBool>,
        output_dir: Option<String>,
        progress: &mut F,
    ) -> Result<CompressionResponse, AppError>
    where
        F: FnMut(ProgressUpdate),
    {
        let attempt_settings = CompressionSettings {
            image_quality: quality,
            max_image_size_px: edge,
            output_dir,
            ..settings.clone()
        };
        compress_pdf_with_progress(path, attempt_settings, Arc::clone(cancel_flag), |update| {
            progress(update)
        })
    }

    let mut lo = i32::from(MIN_SEARCH_QUALITY);
    let mut hi = i32::from(settings.image_quality.max(MIN_SEARCH_QUALITY));
    let mut edge = settings.max_image_size_px;
    let mut met: Option<(u8, u16)> = None;
    let mut attempts = 0usize;

    while attempts < MAX_ATTEMPTS {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled(path.to_string()));
        }

        // After three misses the quality lever alone is not enough — start
        // shrinking the resolution as well.
        let quality = if attempts == 0 { hi } else { (lo + hi) / 2 };
        if attempts >= 3 {
            edge = ((u32::from(edge) * 3 / 4).max(u32::from(MIN_SEARCH_EDGE))) as u16;
        }
        attempts += 1;

        report_progress(ProgressUpdate::new("compressing", 8.0).with_message(
            BackendNotice::new(
                "compress.note.targetAttempt",
                "neutral",
                format!(
                    "Fitting to the target size: trying JPEG quality {quality} (attempt {attempts})."
                ),
            )
            .with_value("quality", quality.to_string())
            .with_value("attempt", attempts.to_string()),
        ));

        let response = probe(
            path,
            quality as u8,
            edge,
            &settings,
            &cancel_flag,
            Some(scratch_dir.clone()),
            report_progress,
        )?;

        if response.compressed_size_bytes <= target_bytes as f64 {
            met = Some((quality as u8, edge));
            break;
        }

        lo = quality + 1;
        if lo > hi {
            // Quality floor exhausted — reset the range so later attempts
            // probe the floor quality with a shrinking edge.
            lo = i32::from(MIN_SEARCH_QUALITY);
            hi = i32::from(MIN_SEARCH_QUALITY);
        }
    }

    let (final_quality, final_edge, target_met) = met
        .map(|(quality, edge)| (quality, edge, true))
        .unwrap_or_else(|| (MIN_SEARCH_QUALITY, edge, false));

    // Final pass with the winning (or best-effort) parameters, written next
    // to the source with the regular output naming.
    let mut response = probe(
        path,
        final_quality,
        final_edge,
        &settings,
        &cancel_flag,
        None,
        report_progress,
    )?;

    let target_kb = target_bytes / 1024;
    let notice = if target_met {
        BackendNotice::new(
            "compress.note.targetSizeMet",
            "success",
            format!("Met the {target_kb} KB target size with JPEG quality {final_quality}."),
        )
        .with_value("targetKb", target_kb.to_string())
        .with_value("quality", final_quality.to_string())
    } else {
        BackendNotice::new(
            "compress.warning.targetSizeMissed",
            "warning",
            format!(
                "Could not reach the {target_kb} KB target; produced the best achievable result instead."
            ),
        )
        .with_value("targetKb", target_kb.to_string())
    };
    response.notices.insert(0, notice);

    Ok(response)
}
