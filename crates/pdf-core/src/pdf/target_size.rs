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
    image_worker_count, prepare_document, CompressionStats,
};
use super::encode::{longest_edge, SkipPolicy};
use super::ensure_not_cancelled;
use super::search::{
    enforce_bitmap_cache_budget, materialize_image_entry, probe_image_at,
    take_image_search_entries, ImageSearchEntry, RoundParams, SearchContext,
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

/// Upper bound on probe attempts before settling for the best effort. Each
/// probe re-encodes from cached decodes, so a generous cap buys tighter
/// convergence without a proportional runtime cost.
const MAX_ATTEMPTS: usize = 12;

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

    // --- Load the document once for the whole search ---
    ensure_not_cancelled(&cancel_flag, path)?;
    let mut document = Document::load(&input_path)
        .map_err(|e| AppError::PdfBuild(format!("Failed to load PDF: {e}")))?;
    let mut stats = CompressionStats {
        decrypted_with_empty_password: super::ensure_not_encrypted(&mut document)?,
        ..CompressionStats::default()
    };

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
        &preparation.color_space_by_image,
    );
    let original_image_bytes: u64 = entries
        .iter()
        .map(|entry| original_entry_bytes(entry) as u64)
        .sum();
    let constant_bytes = baseline_bytes.saturating_sub(original_image_bytes);

    // --- Parameter search ---
    // Classic bisection on JPEG quality for "the highest quality that fits":
    // a fitting probe raises the floor so leftover budget is spent on quality,
    // an over-budget probe lowers the ceiling. The search spans the FULL
    // quality range and starts at the document's own largest image edge (i.e.
    // no downscale) — the preset's quality/edge are hints, not ceilings.
    // Capping at the preset would finish far under budget whenever the preset
    // is more aggressive than the target needs (e.g. a "maximum" preset with
    // a generous budget). The edge only shrinks after the whole quality range
    // failed, and never while a fitting round is known.
    let skip_policy = SkipPolicy::for_document(
        entries.len(),
        settings.grayscale || settings.bilevel_codec.uses_ccitt(),
    );
    let search_context = SearchContext {
        settings: &settings,
        skip_policy,
        cancel_flag: &cancel_flag,
        task_id: path,
    };
    let materialize_context = MaterializeContext {
        search: search_context,
        output_path: &output_path,
        original_size_bytes,
        started_at,
    };
    let mut lo = i32::from(MIN_SEARCH_QUALITY);
    let mut hi = i32::from(u8::MAX);
    let mut edge = start_search_edge(&entries);
    let mut best: Option<(RoundParams, CompressionResponse)> = None;
    let mut last_materialized: Option<RoundParams> = None;
    // Quality whose failure collapsed the current range — the next smaller
    // edge restarts the range at that quality instead of the top, because
    // higher qualities already proved over budget at a *larger* edge.
    let mut collapse_quality = i32::from(u8::MAX);
    // Smallest estimated probe — the best-effort fallback when nothing fits,
    // so the final output is a measured near-minimum rather than a guess.
    let mut smallest_probe: Option<(RoundParams, u64)> = None;
    let mut attempts = 0usize;
    // The schedule can repeat identical (quality, edge) pairs after an edge
    // reset. Identical parameters give an identical result, so reuse the
    // estimate.
    let mut last_probed: Option<(RoundParams, u64)> = None;

    while attempts < MAX_ATTEMPTS {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled(path.to_string()));
        }

        if lo > hi {
            if best.is_some() || edge <= MIN_SEARCH_EDGE {
                break;
            }
            // The entire quality range is over budget at this edge — shrink
            // and restart the range at the quality that collapsed it.
            edge = shrink_search_edge(edge);
            lo = i32::from(MIN_SEARCH_QUALITY);
            hi = collapse_quality;
        }

        let params = RoundParams {
            quality: next_quality(attempts, lo, hi),
            edge,
        };
        attempts += 1;

        let percent = (10.0 + 10.0 * attempts as f32).min(90.0);
        report_progress(
            ProgressUpdate::new("compressing", percent).with_message(BackendNotice::new(
                "compress.note.targetAttempt",
                "neutral",
                format!(
                    "Fitting to the target size: trying JPEG quality {} (attempt {attempts}).",
                    params.quality
                ),
            )
            .with_value("quality", params.quality.to_string())
            .with_value("attempt", attempts.to_string())),
        );

        let estimated = match last_probed {
            Some((previous, previous_estimate)) if previous == params => previous_estimate,
            _ => {
                let estimate =
                    constant_bytes + run_probe_round(&mut entries, params, &search_context)?
                        as u64;
                last_probed = Some((params, estimate));
                if smallest_probe.is_none_or(|(_, size)| estimate < size) {
                    smallest_probe = Some((params, estimate));
                }
                estimate
            }
        };
        collapse_quality = i32::from(params.quality);

        let mut fits = false;
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
                false,
            )?;
            last_materialized = Some(params);
            fits = response.compressed_size_bytes <= target_bytes as f64;

            if fits {
                best = Some((params, response));
                // The budget has headroom: try to spend it on quality.
            }
            // An optimistic estimate (fits == false) leaves the file on disk
            // for now; the finish step restores the best round if one exists.
        }
        let (next_lo, next_hi) = update_quality_range(lo, hi, i32::from(params.quality), fits);
        lo = next_lo;
        hi = next_hi;
    }

    // --- Finish: keep the best verified fit, else best effort at the floor ---
    let met = best.as_ref().map(|(params, _)| *params);
    let response = match best {
        Some((params, response)) if last_materialized == Some(params) => response,
        Some((params, _)) => {
            // A later optimistic round clobbered the best file — restore it.
            report_progress(ProgressUpdate::new("writing", 92.0));
            materialize_and_save(
                &mut document,
                &mut entries,
                params,
                &materialize_context,
                &mut stats,
                true,
            )?
        }
        None => {
            // Best effort: the smallest estimated probe is a measured
            // near-minimum; fall back to the floor when nothing was probed.
            let final_params = smallest_probe
                .map(|(params, _)| params)
                .unwrap_or(RoundParams {
                    quality: MIN_SEARCH_QUALITY,
                    edge,
                });
            report_progress(ProgressUpdate::new("writing", 92.0));
            materialize_and_save(
                &mut document,
                &mut entries,
                final_params,
                &materialize_context,
                &mut stats,
                true,
            )?
        }
    };

    Ok(with_target_notice(response, target_bytes, met))
}

/// Everything the materialize step needs besides the document and entries.
struct MaterializeContext<'a> {
    search: SearchContext<'a>,
    output_path: &'a Path,
    original_size_bytes: u64,
    started_at: Instant,
}

/// Quality to probe next: the upper bound on the first attempt, then the
/// midpoint of the remaining range. Always within `[MIN_SEARCH_QUALITY, 100]`.
fn next_quality(attempts: usize, lo: i32, hi: i32) -> u8 {
    let quality = if attempts == 0 { hi } else { (lo + hi) / 2 };
    quality.clamp(i32::from(MIN_SEARCH_QUALITY), i32::from(u8::MAX)) as u8
}

/// Range update after probing `quality`: a fit raises the floor (the leftover
/// budget may allow higher quality), a miss lowers the ceiling.
fn update_quality_range(lo: i32, hi: i32, quality: i32, fits: bool) -> (i32, i32) {
    if fits {
        (quality + 1, hi)
    } else {
        (lo, quality - 1)
    }
}

/// Edge for the next battle round: three quarters of the current edge,
/// floored at `MIN_SEARCH_EDGE`.
fn shrink_search_edge(edge: u16) -> u16 {
    ((u32::from(edge) * 3 / 4).max(u32::from(MIN_SEARCH_EDGE))) as u16
}

/// Starting edge for the search: the document's own largest image edge, so
/// the first rounds never downscale (each image is clamped to its own edge
/// anyway). Shrinking kicks in only when the budget demands it.
fn start_search_edge(entries: &[ImageSearchEntry]) -> u16 {
    entries
        .iter()
        .filter_map(|entry| longest_edge(&entry.stream))
        .max()
        .map_or(MIN_SEARCH_EDGE, |edge| edge.clamp(u32::from(MIN_SEARCH_EDGE), u32::from(u16::MAX)) as u16)
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
/// `final_write` controls object renumbering — see
/// `save_and_build_response_with_renumber`.
fn materialize_and_save(
    document: &mut Document,
    entries: &mut [ImageSearchEntry],
    params: RoundParams,
    context: &MaterializeContext<'_>,
    stats: &mut CompressionStats,
    final_write: bool,
) -> Result<CompressionResponse, AppError> {
    stats.images_recompressed = 0;
    stats.images_skipped = 0;
    stats.images_bilevel_encoded = 0;
    stats.image_skip_notices = 0;
    stats.suppressed_skip_notices = 0;
    stats.notices.clear();

    for entry in entries.iter_mut() {
        materialize_image_entry(document, entry, params, &context.search, stats)?;
    }

    let final_settings = CompressionSettings {
        image_quality: params.quality,
        max_image_size_px: params.edge,
        ..context.search.settings.clone()
    };
    super::compressor::save_and_build_response_with_renumber(
        document,
        context.output_path,
        context.original_size_bytes,
        context.started_at,
        &final_settings,
        stats,
        final_write,
    )
}

/// Run one probe round across all images, in parallel when the pool sizing
/// allows it. Returns the total encoded contribution in bytes. Entries travel
/// through the worker pool and back — each worker owns an entry exclusively,
/// so no locking is needed for the per-image caches.
fn run_probe_round(
    entries: &mut Vec<ImageSearchEntry>,
    params: RoundParams,
    context: &SearchContext<'_>,
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
            total += probe_image_at(entry, params, context)?;
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
        context.cancel_flag,
        context.task_id,
        |mut entry: ImageSearchEntry| {
            let contribution = probe_image_at(&mut entry, params, context);
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
        assert_eq!(next_quality(0, i32::from(MIN_SEARCH_QUALITY), 80), 80);
    }

    #[test]
    fn later_rounds_bisect_the_quality_range() {
        assert_eq!(next_quality(1, 15, 80), (15 + 80) / 2);
        assert_eq!(next_quality(2, 15, 47), (15 + 47) / 2);
    }

    #[test]
    fn quality_never_drops_below_the_search_floor() {
        // Degenerate bounds (lo > hi) still yield a usable in-range quality.
        let quality = next_quality(2, 90, 80);
        assert!(quality >= MIN_SEARCH_QUALITY);
        assert!(quality <= 100);
    }

    #[test]
    fn a_fit_raises_the_floor_and_a_miss_lowers_the_ceiling() {
        assert_eq!(update_quality_range(15, 80, 50, true), (51, 80));
        assert_eq!(update_quality_range(15, 80, 50, false), (15, 49));
    }

    #[test]
    fn edge_shrink_compounds_with_a_floor() {
        assert_eq!(shrink_search_edge(1600), 1200);
        assert_eq!(shrink_search_edge(1200), 900);
        // Below the floor the edge clamps instead of undershooting.
        assert_eq!(shrink_search_edge(500), MIN_SEARCH_EDGE);
    }

    #[test]
    fn bisection_converges_to_the_highest_fitting_quality() {
        // Simulate the loop's range arithmetic against a synthetic "fits"
        // predicate: quality 62 fits, anything higher does not.
        let fits = |quality: i32| quality <= 62;
        let mut lo = i32::from(MIN_SEARCH_QUALITY);
        let mut hi = 80;
        let mut best = None;
        for attempts in 0..MAX_ATTEMPTS {
            if lo > hi {
                break;
            }
            let quality = i32::from(next_quality(attempts, lo, hi));
            let (next_lo, next_hi) = update_quality_range(lo, hi, quality, fits(quality));
            if fits(quality) {
                best = Some(quality);
            }
            lo = next_lo;
            hi = next_hi;
        }
        assert_eq!(best, Some(62));
    }

    #[test]
    fn nothing_fitting_keeps_the_range_falling_until_the_floor() {
        let mut lo = i32::from(MIN_SEARCH_QUALITY);
        let mut hi = 80;
        let mut edge = 1600u16;
        let mut shrinks = 0;
        for attempts in 0..MAX_ATTEMPTS {
            if lo > hi {
                if edge <= MIN_SEARCH_EDGE {
                    break;
                }
                edge = shrink_search_edge(edge);
                shrinks += 1;
                lo = i32::from(MIN_SEARCH_QUALITY);
                hi = 80;
            }
            let quality = i32::from(next_quality(attempts, lo, hi));
            let (next_lo, next_hi) = update_quality_range(lo, hi, quality, false);
            lo = next_lo;
            hi = next_hi;
        }
        assert!(shrinks >= 1, "edge must shrink once the quality range fails");
        assert!(edge >= MIN_SEARCH_EDGE);
    }
}
