//! Target-size compression — search quality/edge parameters until the output
//! fits a byte budget, or return the best effort.
//! 目标大小压缩 — 搜索质量/边长参数直到输出满足字节预算，否则尽力而为。
//!
//! The search avoids re-running the full pipeline per probe:
//! 1. The document is loaded and prepared **once** (dedupe, stream
//!    compression, metadata removal) — those parts never change between
//!    probes.
//! 2. A baseline serialization of the prepared document fixes the size of the
//!    non-image remainder plus the per-object framing, a constant across
//!    rounds.
//! 3. Each probe round only re-encodes images — decoded planes are cached
//!    across rounds because JPEG decode results do not depend on quality —
//!    and sums the encoded bytes in memory. No disk writes until the end.
//! 4. The winning parameters are materialized once next to the source file
//!    and verified against the budget; if the estimate was optimistic, the
//!    search tightens and continues, keeping the written file as best effort.

use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

use lopdf::Document;

use super::compressor::{
    build_output_path, ensure_input_size_supported, estimated_decoded_bitmap_bytes,
    image_worker_count, prepare_document, save_and_build_response, CompressionStats,
};
use super::ensure_not_cancelled;
use super::search::{
    enforce_bitmap_cache_budget, materialize_image_entry, probe_image_at,
    take_image_search_entries, ImageSearchEntry, RoundParams,
};
use super::settings::CompressionSettings;
use super::workers::run_worker_pool;
use crate::{
    error::AppError,
    models::{BackendNotice, CompressionResponse, ProgressUpdate},
};

/// Lower bound of the quality binary search. Below this, JPEG artifacts
/// destroy the document; the edge shrink takes over instead.
const MIN_SEARCH_QUALITY: u8 = 15;

/// Upper bound on probe attempts before settling for the best effort.
const MAX_ATTEMPTS: usize = 6;

/// Never shrink the max edge below this while fighting for a budget.
const MIN_SEARCH_EDGE: u16 = 400;

/// Compress `path` so the output fits within `target_bytes` when possible.
///
/// Probes run in memory (see the module docs); when the budget cannot be met,
/// the smallest achievable result is produced with a warning notice.
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

    let started_at = Instant::now();
    ensure_not_cancelled(&cancel_flag, path)?;
    report_progress(ProgressUpdate::new("compressing", 5.0));

    // --- Validate input & read file metadata ---
    let input_path = super::validate_input_path(path)?;
    let original_size_bytes = fs::metadata(&input_path)?.len();
    ensure_input_size_supported(original_size_bytes)?;
    // The final file lands next to the source with the regular naming.
    let output_path = build_output_path(&input_path, &settings)?;
    let materialize_context = MaterializeContext {
        settings: &settings,
        cancel_flag: &cancel_flag,
        task_id: path,
        output_path: &output_path,
        original_size_bytes,
        started_at,
    };

    // --- Load the document once for the whole search ---
    ensure_not_cancelled(&cancel_flag, path)?;
    let mut document = Document::load(&input_path)
        .map_err(|e| AppError::PdfBuild(format!("Failed to load PDF: {e}")))?;
    let mut stats = CompressionStats::default();

    // --- Preparation shared by every probe round ---
    let preparation = prepare_document(
        &mut document,
        &settings,
        &cancel_flag,
        &mut stats,
        report_progress,
        path,
    )?;

    // --- Baseline: the prepared document with images still in place ---
    // Non-image content and per-object framing are constant across rounds,
    // so one serialization fixes the additive constant of the size model.
    // Probes then only sum the (re-)encoded image bytes. Pruning (but not
    // renumbering — the scan's object ids must stay valid for the take below)
    // matches the final save's object set; the residual id-width drift is a
    // few bytes per object and covered by the post-materialize verification.
    ensure_not_cancelled(&cancel_flag, path)?;
    document.prune_objects();
    let mut baseline = Vec::new();
    document
        .save_modern(&mut baseline)
        .map_err(|e| AppError::PdfBuild(format!("Failed to estimate the output size: {e}")))?;
    let baseline_bytes = baseline.len() as u64;
    drop(baseline);

    // --- Move the image objects out as cached search entries ---
    let mut entries = take_image_search_entries(
        &mut document,
        &preparation.image_object_ids,
        &preparation.shared_smask_ids,
    );
    let original_image_bytes: u64 = entries
        .iter()
        .map(|entry| original_entry_bytes(entry) as u64)
        .sum();
    let constant_bytes = baseline_bytes.saturating_sub(original_image_bytes);

    // --- Parameter search ---
    let mut lo = i32::from(MIN_SEARCH_QUALITY);
    let mut hi = i32::from(settings.image_quality.max(MIN_SEARCH_QUALITY));
    let mut edge = settings.max_image_size_px;
    let mut met: Option<RoundParams> = None;
    let mut attempts = 0usize;
    // The schedule can repeat identical (quality, edge) pairs — e.g. the
    // floor quality twice while waiting for the edge shrink to kick in.
    // Identical parameters give an identical result, so reuse the estimate.
    let mut last_probed: Option<(RoundParams, u64)> = None;

    while attempts < MAX_ATTEMPTS {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled(path.to_string()));
        }

        // The quality/edge schedule for this attempt (pure — see below).
        let params = round_params(attempts, lo, hi, edge);
        let quality = i32::from(params.quality);
        edge = params.edge;
        attempts += 1;

        report_progress(
            ProgressUpdate::new("compressing", 10.0 + 12.0 * attempts as f32).with_message(
                BackendNotice::new(
                    "compress.note.targetAttempt",
                    "neutral",
                    format!(
                        "Fitting to the target size: trying JPEG quality {quality} (attempt {attempts})."
                    ),
                )
                .with_value("quality", quality.to_string())
                .with_value("attempt", attempts.to_string()),
            ),
        );

        let estimated = match last_probed {
            Some((previous, previous_estimate)) if previous == params => previous_estimate,
            _ => {
                let estimate = constant_bytes
                    + run_probe_round(&mut entries, params, &settings, &cancel_flag, path)? as u64;
                last_probed = Some((params, estimate));
                estimate
            }
        };

        if estimated <= target_bytes {
            // Materialize the round and verify against the real budget —
            // the estimate ignores per-image dictionary shape shifts of a
            // few dozen bytes, so trust the file, not the model.
            report_progress(ProgressUpdate::new("writing", 92.0));
            let response = materialize_and_save(
                &mut document,
                &mut entries,
                params,
                &materialize_context,
                &mut stats,
            )?;

            if response.compressed_size_bytes <= target_bytes as f64 {
                met = Some(params);
                return Ok(with_target_notice(response, target_bytes, met));
            }

            // Optimistic estimate: keep the written file as best effort and
            // continue the search with tightened bounds.
        }

        let (next_lo, next_hi) = tighten_bounds(lo, hi, quality);
        lo = next_lo;
        hi = next_hi;
    }

    // --- Best effort at the floor parameters ---
    let final_params = met.unwrap_or(RoundParams {
        quality: MIN_SEARCH_QUALITY,
        edge,
    });

    report_progress(ProgressUpdate::new("writing", 92.0));
    let response = materialize_and_save(
        &mut document,
        &mut entries,
        final_params,
        &materialize_context,
        &mut stats,
    )?;

    Ok(with_target_notice(response, target_bytes, met))
}

/// Everything the materialize step needs besides the document and entries.
struct MaterializeContext<'a> {
    settings: &'a CompressionSettings,
    cancel_flag: &'a Arc<AtomicBool>,
    task_id: &'a str,
    output_path: &'a Path,
    original_size_bytes: u64,
    started_at: Instant,
}

/// Quality/edge parameters for attempt `attempts` (0-based) under the current
/// bounds: the first attempt probes the upper quality bound, later attempts
/// bisect; from the fourth attempt the edge shrinks to ¾ (floored).
fn round_params(attempts: usize, lo: i32, hi: i32, mut edge: u16) -> RoundParams {
    let quality = if attempts == 0 { hi } else { (lo + hi) / 2 };
    if attempts >= 3 {
        edge = ((u32::from(edge) * 3 / 4).max(u32::from(MIN_SEARCH_EDGE))) as u16;
    }
    RoundParams {
        quality: quality.clamp(i32::from(MIN_SEARCH_QUALITY), i32::from(u8::MAX)) as u8,
        edge,
    }
}

/// Binary-search bound update after probing `quality`: move the floor above
/// it, or reset both bounds to the floor once the quality range is exhausted
/// (later attempts then probe the floor quality with a shrinking edge).
/// The current floor is irrelevant — it is always replaced.
fn tighten_bounds(_lo: i32, hi: i32, quality: i32) -> (i32, i32) {
    let lo = quality + 1;
    if lo > hi {
        (i32::from(MIN_SEARCH_QUALITY), i32::from(MIN_SEARCH_QUALITY))
    } else {
        (lo, hi)
    }
}

/// Untouched byte footprint of an entry's streams.
fn original_entry_bytes(entry: &ImageSearchEntry) -> usize {
    entry.stream.content.len()
        + entry
            .smask
            .as_ref()
            .map_or(0, |(_, mask)| mask.content.len())
}

/// Apply one round of parameters to the document and write the output file.
/// Image counters are reset first: a previous over-budget materialization
/// already counted its images, and stats must reflect the final parameters.
fn materialize_and_save(
    document: &mut Document,
    entries: &mut [ImageSearchEntry],
    params: RoundParams,
    context: &MaterializeContext<'_>,
    stats: &mut CompressionStats,
) -> Result<CompressionResponse, AppError> {
    stats.images_recompressed = 0;
    stats.images_skipped = 0;
    stats.image_skip_notices = 0;
    stats.suppressed_skip_notices = 0;
    stats.notices.clear();

    for entry in entries.iter_mut() {
        materialize_image_entry(
            document,
            entry,
            params,
            context.settings,
            context.cancel_flag,
            context.task_id,
            stats,
        )?;
    }

    let final_settings = CompressionSettings {
        image_quality: params.quality,
        max_image_size_px: params.edge,
        ..context.settings.clone()
    };
    save_and_build_response(
        document,
        context.output_path,
        context.original_size_bytes,
        context.started_at,
        &final_settings,
        stats,
    )
}

/// Run one probe round across all images, in parallel when the pool sizing
/// allows it. Returns the total encoded contribution in bytes. Entries travel
/// through the worker pool and back — each worker owns an entry exclusively,
/// so no locking is needed for the per-image caches.
fn run_probe_round(
    entries: &mut Vec<ImageSearchEntry>,
    params: RoundParams,
    settings: &CompressionSettings,
    cancel_flag: &Arc<AtomicBool>,
    task_id: &str,
) -> Result<usize, AppError> {
    let count = entries.len().max(1);
    let max_bitmap_estimate = entries
        .iter()
        .map(|entry| estimated_decoded_bitmap_bytes(&entry.stream))
        .max()
        .unwrap_or(0);
    let worker_count = image_worker_count(count, max_bitmap_estimate);

    if entries.len() < 2 || worker_count <= 1 {
        let mut total = 0usize;
        for entry in entries.iter_mut() {
            total += probe_image_at(entry, params, settings, cancel_flag, task_id)?;
        }
        enforce_bitmap_cache_budget(entries);
        return Ok(total);
    }

    let queue = std::mem::take(entries);
    let mut total = 0usize;
    let mut collected = Vec::new();

    run_worker_pool(
        queue,
        worker_count,
        worker_count,
        cancel_flag,
        task_id,
        |mut entry: ImageSearchEntry| {
            let contribution = probe_image_at(&mut entry, params, settings, cancel_flag, task_id);
            (entry, contribution)
        },
        |(entry, contribution)| {
            total += contribution?;
            collected.push(entry);
            Ok(())
        },
    )?;

    enforce_bitmap_cache_budget(&mut collected);
    *entries = collected;
    Ok(total)
}

/// Prepend the target-met / best-effort notice to the response.
fn with_target_notice(
    mut response: CompressionResponse,
    target_bytes: u64,
    met: Option<RoundParams>,
) -> CompressionResponse {
    let target_kb = target_bytes / 1024;
    let notice = match met {
        Some(params) => BackendNotice::new(
            "compress.note.targetSizeMet",
            "success",
            format!(
                "Met the {target_kb} KB target size with JPEG quality {}.",
                params.quality
            ),
        )
        .with_value("targetKb", target_kb.to_string())
        .with_value("quality", params.quality.to_string()),
        None => BackendNotice::new(
            "compress.warning.targetSizeMissed",
            "warning",
            format!(
                "Could not reach the {target_kb} KB target; produced the best achievable result instead."
            ),
        )
        .with_value("targetKb", target_kb.to_string()),
    };
    response.notices.insert(0, notice);
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_round_probes_the_upper_quality_bound() {
        let params = round_params(0, i32::from(MIN_SEARCH_QUALITY), 80, 1600);
        assert_eq!(params.quality, 80);
        assert_eq!(params.edge, 1600);
    }

    #[test]
    fn later_rounds_bisect_the_quality_range_without_touching_the_edge() {
        let params = round_params(1, 15, 80, 1600);
        assert_eq!(params.quality, (15 + 80) / 2);
        assert_eq!(params.edge, 1600);

        let early = round_params(2, 15, 47, 1600);
        assert_eq!(early.edge, 1600);
    }

    #[test]
    fn edge_shrinks_from_the_fourth_attempt_with_a_floor() {
        let params = round_params(3, 15, 80, 1600);
        assert_eq!(params.edge, 1200);

        // Shrinks compound attempt over attempt: 1200 → 900 → 675 → 506 → 400…
        let compounded = round_params(7, 15, 15, 1200);
        assert_eq!(compounded.edge, 900);

        // Below the floor the edge clamps instead of undershooting.
        let floored = round_params(6, 15, 15, 500);
        assert_eq!(floored.edge, MIN_SEARCH_EDGE);
    }

    #[test]
    fn quality_never_drops_below_the_search_floor() {
        // Degenerate bounds (lo > hi) still yield a usable in-range quality.
        let params = round_params(2, 90, 80, 1600);
        assert!(params.quality >= MIN_SEARCH_QUALITY);
        assert!(params.quality <= 100);
    }

    #[test]
    fn bounds_move_up_after_a_miss_and_reset_once_exhausted() {
        assert_eq!(tighten_bounds(15, 80, 50), (51, 80));
        // quality = hi exhausts the range → reset to the floor so later
        // attempts probe MIN_SEARCH_QUALITY with a shrinking edge.
        assert_eq!(
            tighten_bounds(15, 80, 80),
            (
                i32::from(MIN_SEARCH_QUALITY),
                i32::from(MIN_SEARCH_QUALITY)
            )
        );
    }

    #[test]
    fn a_full_schedule_respects_the_attempt_cap_and_shrinks_the_edge() {
        let mut lo = i32::from(MIN_SEARCH_QUALITY);
        let mut hi = 80;
        let mut edge = 1600u16;
        let mut last = None;

        for attempts in 0..MAX_ATTEMPTS {
            let params = round_params(attempts, lo, hi, edge);
            edge = params.edge;
            let (next_lo, next_hi) = tighten_bounds(lo, hi, i32::from(params.quality));
            lo = next_lo;
            hi = next_hi;
            last = Some(params);
        }

        let last = last.expect("schedule produced a round");
        assert!(last.quality >= MIN_SEARCH_QUALITY);
        assert!(last.edge >= MIN_SEARCH_EDGE);
        assert!(last.edge < 1600, "edge must have shrunk by the last attempt");
    }
}
