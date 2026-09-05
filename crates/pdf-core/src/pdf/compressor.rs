//! Object-level PDF compression orchestration — the document walk, batch
//! scheduling, stream compression, and metadata removal. Per-image codec
//! logic lives in `encode.rs`, target-size search state in `search.rs`, and
//! the shared worker pool in `workers.rs`.
//! 对象级 PDF 压缩编排 — 文档遍历、批量调度、流压缩与元数据移除。
//! 单图编解码逻辑位于 encode.rs，目标大小搜索状态位于 search.rs，
//! 共享线程池位于 workers.rs。

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Instant,
};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use super::encode::{optimize_image_stream, pixel_count, ImageOptimization, SkipPolicy};
use super::ensure_not_cancelled;
use super::settings::CompressionSettings;
use super::workers::{run_worker_pool, CHANNEL_BUFFER_MULTIPLIER};
use super::validate_input_path;
use crate::{
    error::AppError,
    models::{BackendNotice, CompressionResponse, ProgressUpdate},
};

/// Progress range: object scan phase.
const OBJECT_SCAN_PROGRESS_START: f32 = 15.0;
const OBJECT_SCAN_PROGRESS_END: f32 = 38.0;

/// Progress range: image optimization phase.
const IMAGE_OPTIMIZATION_PROGRESS_END: f32 = 88.0;

/// Maximum number of worker threads for parallel image recompression.
/// Capped to avoid excessive memory consumption from concurrent large-image decodes.
const MAX_IMAGE_WORKERS: usize = 8;

/// Minimum image objects before enabling parallel processing.
/// Below this count, thread pool overhead outweighs the gains.
const MIN_PARALLEL_IMAGE_OBJECTS: usize = 3;

/// Non-image streams shorter than this are too small for compression to help.
/// Lowered from 128 to 64 bytes — deflate can still win on repetitive short streams.
const MIN_COMPRESSIBLE_STREAM_BYTES: usize = 64;

/// Inputs above this size are rejected: the whole document is loaded into
/// memory, so a hard ceiling protects against OOM on multi-gigabyte files.
const MAX_INPUT_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB

/// Maximum number of per-image skip notices included in the response.
/// Additional skips are counted but not individually reported.
const MAX_IMAGE_SKIP_NOTICES: usize = 6;

/// Guard against OOM on oversized inputs. Shared by analyze and compress.
pub(crate) fn ensure_input_size_supported(size_bytes: u64) -> Result<(), AppError> {
    if size_bytes > MAX_INPUT_BYTES {
        return Err(AppError::PdfBuild(format!(
            "Input file is too large to process safely ({size_bytes} bytes; the limit is {MAX_INPUT_BYTES} bytes)."
        )));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub(crate) struct CompressionStats {
    pub(crate) images_recompressed: usize,
    pub(crate) images_skipped: usize,
    pub(crate) images_deduplicated: usize,
    /// Non-image streams merged into a canonical object (content streams,
    /// font programs, form XObjects).
    pub(crate) streams_deduplicated: usize,
    /// `/Font` and `/XObject` resource entries removed because no content
    /// stream in the owning tree referenced them.
    pub(crate) resources_removed: usize,
    /// Embedded Type0/CIDFontType2 fonts shrunk to their used glyphs.
    pub(crate) fonts_subsetted: usize,
    /// Font program bytes shed by subsetting.
    pub(crate) font_bytes_saved: u64,
    /// Recompressed images whose rebuilt stream carries CCITT Group 4.
    pub(crate) images_bilevel_encoded: usize,
    pub(crate) streams_compressed: usize,
    pub(crate) metadata_removed: bool,
    /// The input carried `/Encrypt` but lopdf unlocked it with the empty user
    /// password (owner-password-only restrictions); the output is plain.
    pub(crate) decrypted_with_empty_password: bool,
    pub(crate) notices: Vec<BackendNotice>,
    pub(crate) image_skip_notices: usize,
    pub(crate) suppressed_skip_notices: usize,
}

#[derive(Debug)]
pub(super) struct ImageTask {
    pub(super) object_id: ObjectId,
    pub(super) stream: Stream,
    /// Soft-mask stream referenced by `/SMask`, moved out of the document on
    /// the main thread (workers have no document access) as
    /// `(original object id, stream)`. `None` when absent.
    pub(super) smask: Option<(ObjectId, Stream)>,
    /// Resolved color-space context (ICC/Indexed/aliases); `None` means the
    /// stream dictionary alone describes the pixels.
    pub(super) color_space: Option<super::colorspace::ImageColorSpaceInfo>,
    /// Cached stream byte length — avoids re-reading during scheduling.
    stream_size: usize,
}

#[derive(Debug)]
struct ImageTaskOutcome {
    object_id: ObjectId,
    result: Result<ImageOptimization, AppError>,
    /// Originals returned by the worker so the main thread can restore them
    /// untouched when the outcome is a skip.
    stream: Stream,
    smask: Option<(ObjectId, Stream)>,
}

#[derive(Clone, Copy)]
struct CompressionRuntime<'a> {
    cancel_flag: &'a Arc<AtomicBool>,
    task_id: &'a str,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Compress a PDF file according to the given settings, reporting progress via
/// the provided callback. Returns detailed statistics on what was optimized.
///
/// Encrypted inputs: owner-password-only files unlock automatically (the
/// output is plain, reported via a notice); files that need an open password
/// fail with `AppError::PasswordRequired` (no password given) or
/// `AppError::WrongPassword`; DRM-encrypted files fail with
/// `AppError::Encrypted`. An optimization that would not beat the original
/// writes nothing and reports it.
/// 按设置压缩 PDF 并回报进度与统计；加密文档矩阵与“不写更大输出”语义见上。
pub fn compress_pdf_with_progress<F>(
    path: &str,
    password: Option<&str>,
    settings: CompressionSettings,
    cancel_flag: Arc<AtomicBool>,
    mut report_progress: F,
) -> Result<CompressionResponse, AppError>
where
    F: FnMut(ProgressUpdate),
{
    let started_at = Instant::now();
    ensure_not_cancelled(&cancel_flag, path)?;
    report_progress(ProgressUpdate::new("compressing", 5.0));

    // --- Validate input & read file metadata ---
    let input_path = validate_input_path(path)?;
    let original_size_bytes = fs::metadata(&input_path)?.len();
    ensure_input_size_supported(original_size_bytes)?;
    let output_path = build_output_path(&input_path, &settings)?;

    // --- Load the PDF document ---
    ensure_not_cancelled(&cancel_flag, path)?;
    let password_attempted = password.is_some_and(|value| !value.is_empty());
    let mut document = super::load_document(&input_path, password)?;
    let mut stats = CompressionStats {
        decrypted_with_empty_password: super::ensure_not_encrypted(
            &mut document,
            password_attempted,
        )?,
        ..CompressionStats::default()
    };

    report_progress(ProgressUpdate::new(
        "compressing",
        OBJECT_SCAN_PROGRESS_START,
    ));

    // --- Core optimization pass ---
    optimize_document(
        &mut document,
        &settings,
        &cancel_flag,
        &mut stats,
        &mut report_progress,
        path,
    )?;

    report_progress(ProgressUpdate::new("writing", 92.0));
    ensure_not_cancelled(&cancel_flag, path)?;

    // --- Cleanup, write output & build response ---
    let response = save_and_build_response(
        &mut document,
        &output_path,
        original_size_bytes,
        started_at,
        &settings,
        &mut stats,
    )?;

    report_progress(
        ProgressUpdate::new("done", 100.0).with_message(BackendNotice::new(
            "compress.progress.done",
            "success",
            "Compression finished.",
        )),
    );

    Ok(response)
}

/// Serialize the optimized document, compute result metrics, and assemble the
/// response with the standard notices. Shared by the single-pass compressor
/// and the target-size search materialization.
pub(crate) fn save_and_build_response(
    document: &mut Document,
    output_path: &Path,
    original_size_bytes: u64,
    started_at: Instant,
    settings: &CompressionSettings,
    stats: &mut CompressionStats,
) -> Result<CompressionResponse, AppError> {
    save_and_build_response_with_renumber(
        document,
        output_path,
        original_size_bytes,
        started_at,
        settings,
        stats,
        true,
    )
}

/// `renumber_objects` must be `false` for intermediate target-size probe
/// rounds: renumbering invalidates the object ids the search entries still
/// hold, so a later round (or the best-round restore) would insert streams at
/// stale ids. Only the final, kept write renumbers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn save_and_build_response_with_renumber(
    document: &mut Document,
    output_path: &Path,
    original_size_bytes: u64,
    started_at: Instant,
    settings: &CompressionSettings,
    stats: &mut CompressionStats,
    renumber_objects: bool,
) -> Result<CompressionResponse, AppError> {
    // --- Cleanup & serialize in memory ---
    document.prune_objects();
    if renumber_objects {
        document.renumber_objects();
    }

    let mut serialized = Vec::new();
    document
        .save_modern(&mut serialized)
        .map_err(|e| AppError::PdfBuild(format!("Failed to save optimized PDF: {e}")))?;
    let compressed_size_bytes = serialized.len() as u64;

    // --- Build result notices ---
    stats.notices.insert(
        0,
        BackendNotice::new(
            "compress.note.appliedProfile",
            "neutral",
            format!(
                "Applied the '{}' profile with JPEG quality {} and max image edge {} px.",
                settings.preset.as_label(),
                settings.image_quality,
                settings.max_image_size_px
            ),
        )
        .with_value("preset", settings.preset.as_label())
        .with_value("quality", settings.image_quality.to_string())
        .with_value("maxImageSizePx", settings.max_image_size_px.to_string()),
    );
    stats.notices.push(BackendNotice::new(
        "compress.note.safeRewrite",
        "neutral",
        "The optimizer preserves text and vector instructions when a rewrite is not safe.",
    ));
    if stats.decrypted_with_empty_password {
        stats.notices.push(BackendNotice::new(
            "compress.note.decryptedInput",
            "neutral",
            "The input used owner-password encryption and was read with the empty user \
             password; the output is written unencrypted.",
        ));
    }
    if stats.suppressed_skip_notices > 0 {
        stats.notices.push(
            BackendNotice::new(
                "compress.note.imageSkipSummary",
                "neutral",
                format!(
                    "Suppressed {} additional image skip notices to keep the report concise.",
                    stats.suppressed_skip_notices
                ),
            )
            .with_value("count", stats.suppressed_skip_notices.to_string()),
        );
    }
    if stats.images_deduplicated > 0 {
        stats.notices.push(
            BackendNotice::new(
                "compress.note.imageDedupe",
                "neutral",
                format!(
                    "Merged {} duplicate image objects into shared references.",
                    stats.images_deduplicated
                ),
            )
            .with_value("count", stats.images_deduplicated.to_string()),
        );
    }
    if stats.images_bilevel_encoded > 0 {
        stats.notices.push(
            BackendNotice::new(
                "compress.note.bilevelEncoded",
                "neutral",
                format!(
                    "Re-encoded {} near-black-and-white image(s) as lossless CCITT Group 4.",
                    stats.images_bilevel_encoded
                ),
            )
            .with_value("count", stats.images_bilevel_encoded.to_string()),
        );
    }
    if stats.streams_deduplicated > 0 {
        stats.notices.push(
            BackendNotice::new(
                "compress.note.streamDedupe",
                "neutral",
                format!(
                    "Merged {} duplicate non-image stream object(s) (content, fonts, forms) into shared references.",
                    stats.streams_deduplicated
                ),
            )
            .with_value("count", stats.streams_deduplicated.to_string()),
        );
    }
    if stats.resources_removed > 0 {
        stats.notices.push(
            BackendNotice::new(
                "compress.note.resourcesCleaned",
                "neutral",
                format!(
                    "Removed {} unused font/XObject resource entr{} left behind by earlier edits.",
                    stats.resources_removed,
                    if stats.resources_removed == 1 { "y" } else { "ies" }
                ),
            )
            .with_value("count", stats.resources_removed.to_string()),
        );
    }
    if stats.fonts_subsetted > 0 {
        let saved_kb = (stats.font_bytes_saved as f64 / 1024.0).round() as u64;
        stats.notices.push(
            BackendNotice::new(
                "compress.note.fontsSubsetted",
                "neutral",
                format!(
                    "Subset {} embedded font(s) to their used glyphs, shedding {saved_kb} KB of font data.",
                    stats.fonts_subsetted
                ),
            )
            .with_value("count", stats.fonts_subsetted.to_string())
            .with_value("savedKb", saved_kb.to_string()),
        );
    }
    if !stats.metadata_removed {
        stats.notices.push(BackendNotice::new(
            "compress.note.metadataKept",
            "neutral",
            "Document metadata stayed in place because metadata cleanup was disabled or unavailable.",
        ));
    }

    let elapsed_ms = started_at.elapsed().as_millis().min(u32::MAX as u128) as u32;
    let counters = CompressionResponse {
        output_path: String::new(),
        original_size_bytes: original_size_bytes as f64,
        compressed_size_bytes: compressed_size_bytes as f64,
        saved_bytes: 0.0,
        savings_percent: 0.0,
        elapsed_ms,
        images_recompressed: stats.images_recompressed as u32,
        images_skipped: stats.images_skipped as u32,
        images_deduplicated: stats.images_deduplicated as u32,
        streams_compressed: stats.streams_compressed as u32,
        metadata_removed: stats.metadata_removed,
        output_was_smaller: false,
        notices: std::mem::take(&mut stats.notices),
    };

    // --- Never leave an output that did not improve on the original ---
    // Serialize-then-compare (instead of write-then-stat) so a non-improving
    // result never touches the disk; a file left behind by an earlier
    // optimistic round of the target-size search is removed.
    if original_size_bytes > 0 && compressed_size_bytes >= original_size_bytes {
        let _ = fs::remove_file(output_path);
        let mut response = counters;
        response.notices.insert(
            0,
            BackendNotice::new(
                "compress.warning.outputNotSmaller",
                "warning",
                format!(
                    "Optimization could not beat the original {original_size_bytes} bytes \
                     (best result: {compressed_size_bytes} bytes); nothing was written."
                ),
            )
            .with_value("originalBytes", original_size_bytes.to_string())
            .with_value("bestBytes", compressed_size_bytes.to_string()),
        );
        return Ok(response);
    }

    fs::write(output_path, &serialized)?;

    let saved_bytes = original_size_bytes.saturating_sub(compressed_size_bytes);
    let savings_percent = if original_size_bytes == 0 {
        0.0
    } else {
        (saved_bytes as f32 / original_size_bytes as f32) * 100.0
    };

    Ok(CompressionResponse {
        output_path: output_path.to_string_lossy().to_string(),
        saved_bytes: saved_bytes as f64,
        savings_percent,
        output_was_smaller: true,
        ..counters
    })
}

// ---------------------------------------------------------------------------
// Document-level optimization
// ---------------------------------------------------------------------------

/// Walk every object in the document exactly once:
/// - Collect image stream IDs for batch optimization.
/// - Collect eligible non-image streams for parallel compression.
/// - Optionally strip metadata.
fn optimize_document<F>(
    document: &mut Document,
    settings: &CompressionSettings,
    cancel_flag: &Arc<AtomicBool>,
    stats: &mut CompressionStats,
    report_progress: &mut F,
    task_id: &str,
) -> Result<(), AppError>
where
    F: FnMut(ProgressUpdate),
{
    let runtime = CompressionRuntime {
        cancel_flag,
        task_id,
    };

    let preparation = prepare_document(
        document,
        settings,
        runtime.cancel_flag,
        stats,
        report_progress,
        runtime.task_id,
    )?;

    // --- Batch image optimization (possibly parallel) ---
    if settings.optimize_images && !preparation.image_object_ids.is_empty() {
        let mut image_phase_percent = OBJECT_SCAN_PROGRESS_END;
        optimize_image_streams(
            document,
            &preparation,
            settings,
            runtime,
            stats,
            report_progress,
            &mut image_phase_percent,
        )?;
    } else {
        report_progress(ProgressUpdate::new(
            "compressing",
            IMAGE_OPTIMIZATION_PROGRESS_END,
        ));
    }

    Ok(())
}

/// Scan output: which objects are images, and which soft masks are shared.
pub(crate) struct DocumentPreparation {
    pub image_object_ids: Vec<ObjectId>,
    pub shared_smask_ids: HashSet<ObjectId>,
    /// Document-level color-space context per image (ICC channel counts,
    /// indexed palettes, resolved name aliases).
    pub color_space_by_image: HashMap<ObjectId, super::colorspace::ImageColorSpaceInfo>,
}

/// Shared preparation pass used by both compression entry points: lossless
/// image dedup, soft-mask bookkeeping, stream classification, parallel
/// compression of eligible non-image streams, and metadata removal. Image
/// optimization itself is left to the caller — the target-size search runs it
/// as probe rounds instead of once.
pub(crate) fn prepare_document<F>(
    document: &mut Document,
    settings: &CompressionSettings,
    cancel_flag: &Arc<AtomicBool>,
    stats: &mut CompressionStats,
    report_progress: &mut F,
    task_id: &str,
) -> Result<DocumentPreparation, AppError>
where
    F: FnMut(ProgressUpdate),
{
    // --- Lossless pass: merge fully identical stream objects (images, content
    // streams, font programs, form XObjects) and rewrite incoming references ---
    let (images_merged, streams_merged) = dedupe_identical_streams(document);
    stats.images_deduplicated = images_merged as usize;
    stats.streams_deduplicated = streams_merged as usize;

    // --- Lossless pass: drop /Font and /XObject resource entries no content
    // stream references (conservative — unsafe-looking pages keep everything) ---
    stats.resources_removed = super::resources::remove_unused_resources(document);

    // --- Optional pass: shrink embedded CID TrueType fonts to used glyphs ---
    if settings.subset_fonts {
        #[cfg(feature = "subset-fonts")]
        {
            let outcome =
                super::fonts::subset_embedded_fonts(document, cancel_flag, task_id)?;
            stats.fonts_subsetted += outcome.fonts_subsetted;
            stats.font_bytes_saved += outcome.bytes_saved;
        }
        // Without the `subset-fonts` feature the flag is accepted but inert;
        // the build simply does not contain the subsetting engine.
        #[cfg(not(feature = "subset-fonts"))]
        let _ = (cancel_flag, task_id);
    }

    // Soft-mask streams carry Subtype /Image too — collect the ids referenced
    // as /SMask so the scan treats them as alpha auxiliaries of their parent
    // image instead of standalone optimization targets. Masks referenced by
    // more than one image must stay in the document (they are cloned for the
    // task instead of moved) so every sharer keeps a valid target.
    let mut smask_reference_counts: HashMap<ObjectId, usize> = HashMap::new();
    for object in document.objects.values() {
        let Object::Stream(stream) = object else {
            continue;
        };
        if !is_image_stream(stream) {
            continue;
        }
        if let Some(smask_id) = stream
            .dict
            .get(b"SMask")
            .ok()
            .and_then(|entry| entry.as_reference().ok())
        {
            *smask_reference_counts.entry(smask_id).or_default() += 1;
        }
    }
    let smask_object_ids: HashSet<ObjectId> = smask_reference_counts.keys().copied().collect();
    let shared_smask_ids: HashSet<ObjectId> = smask_reference_counts
        .into_iter()
        .filter_map(|(smask_id, count)| (count > 1).then_some(smask_id))
        .collect();

    let object_ids: Vec<ObjectId> = document.objects.keys().copied().collect();
    let total_objects = object_ids.len().max(1);
    let mut image_object_ids = Vec::new();
    let mut compressible_stream_ids = Vec::new();
    let mut last_reported_percent = OBJECT_SCAN_PROGRESS_START;

    // --- Single-pass scan: classify each object ---
    for (index, object_id) in object_ids.into_iter().enumerate() {
        ensure_not_cancelled(cancel_flag, task_id)?;
        if smask_object_ids.contains(&object_id) {
            report_progress_if_needed(
                report_progress,
                &mut last_reported_percent,
                OBJECT_SCAN_PROGRESS_START,
                OBJECT_SCAN_PROGRESS_END,
                index + 1,
                total_objects,
            );
            continue;
        }
        let Some(object) = document.objects.get_mut(&object_id) else {
            continue;
        };

        let Object::Stream(stream) = object else {
            report_progress_if_needed(
                report_progress,
                &mut last_reported_percent,
                OBJECT_SCAN_PROGRESS_START,
                OBJECT_SCAN_PROGRESS_END,
                index + 1,
                total_objects,
            );
            continue;
        };

        if is_image_stream(stream) {
            if settings.optimize_images {
                image_object_ids.push(object_id);
            }
        } else if settings.compress_streams && stream_is_compressible(stream) {
            compressible_stream_ids.push(object_id);
        }

        report_progress_if_needed(
            report_progress,
            &mut last_reported_percent,
            OBJECT_SCAN_PROGRESS_START,
            OBJECT_SCAN_PROGRESS_END,
            index + 1,
            total_objects,
        );
    }

    // --- Batch compression of eligible non-image streams ---
    // Deflate is CPU-bound; text-heavy PDFs with many uncompressed content
    // streams would otherwise serialize on the scan thread.
    if !compressible_stream_ids.is_empty() {
        stats.streams_compressed +=
            compress_non_image_streams(document, &compressible_stream_ids, cancel_flag, task_id)?;
    }

    // --- Metadata removal ---
    if settings.strip_metadata {
        stats.metadata_removed = remove_metadata(document);
    }

    // --- Color-space context: needs the intact document (ICC streams,
    // indexed lookup tables, resource-dictionary name aliases) ---
    let aliases = super::colorspace::collect_color_space_aliases(document);
    let mut color_space_by_image = HashMap::new();
    for &image_id in &image_object_ids {
        let Some(Object::Stream(stream)) = document.objects.get(&image_id) else {
            continue;
        };
        if let Some(info) =
            super::colorspace::resolve_image_color_space(document, stream, &aliases)
        {
            color_space_by_image.insert(image_id, info);
        }
    }

    Ok(DocumentPreparation {
        image_object_ids,
        shared_smask_ids,
        color_space_by_image,
    })
}

// ---------------------------------------------------------------------------
// Lossless image deduplication
// ---------------------------------------------------------------------------

/// Feed an object into a fingerprint hash stream. Dictionaries are walked in
/// sorted key order so two semantically identical dictionaries with different
/// insertion orders hash identically (lopdf stores entries in an IndexMap).
/// References hash by target id: streams referencing the same object stay
/// merge candidates, streams referencing different objects do not.
fn hash_object_into<H: std::hash::Hasher>(hasher: &mut H, object: &Object) {
    match object {
        Object::Null => hasher.write_u8(0),
        Object::Boolean(value) => {
            hasher.write_u8(1);
            hasher.write_u8(u8::from(*value));
        }
        Object::Integer(value) => {
            hasher.write_u8(2);
            hasher.write_i64(*value);
        }
        Object::Real(value) => {
            hasher.write_u8(3);
            hasher.write_u32(value.to_bits());
        }
        Object::Name(value) => {
            hasher.write_u8(4);
            hasher.write(value);
        }
        Object::String(value, _) => {
            hasher.write_u8(5);
            hasher.write(value);
        }
        Object::Array(items) => {
            hasher.write_u8(6);
            hasher.write_usize(items.len());
            for item in items {
                hash_object_into(hasher, item);
            }
        }
        Object::Dictionary(dict) => {
            hasher.write_u8(7);
            hash_dictionary_into(hasher, dict);
        }
        Object::Stream(stream) => {
            // Streams only appear as top-level objects; hash the dict so a
            // full-object fingerprint stays collision-consistent.
            hasher.write_u8(8);
            hash_dictionary_into(hasher, &stream.dict);
            hasher.write(&stream.content);
        }
        Object::Reference((id, _generation)) => {
            hasher.write_u8(9);
            hasher.write_u32(*id);
        }
    }
}

fn hash_dictionary_into<H: std::hash::Hasher>(hasher: &mut H, dict: &Dictionary) {
    let mut entries: Vec<(&Vec<u8>, &Object)> = dict.iter().collect();
    entries.sort_unstable_by_key(|(key, _)| key.as_slice());
    hasher.write_usize(entries.len());
    for (key, value) in entries {
        hasher.write(key);
        hash_object_into(hasher, value);
    }
}

/// Streaming fingerprint of an arbitrary stream: the full dictionary walked
/// canonically, then the content bytes. FxHash processes word-sized chunks
/// (~10 GB/s vs ~1 GB/s for byte-wise FNV). Hash collisions are never merged:
/// the caller verifies full equality before rewriting references.
fn stream_fingerprint(stream: &Stream) -> u64 {
    use rustc_hash::FxHasher;
    use std::hash::Hasher as _;

    let mut hasher = FxHasher::default();
    hasher.write_u8(8);
    hash_dictionary_into(&mut hasher, &stream.dict);
    hasher.write(&stream.content);
    hasher.finish()
}

/// Order-insensitive deep equality. Mirrors `hash_object_into`: dictionaries
/// compare by key→value regardless of insertion order, references by target
/// id. Used as the full verification after a fingerprint hit.
fn objects_equivalent(left: &Object, right: &Object) -> bool {
    match (left, right) {
        (Object::Null, Object::Null) => true,
        (Object::Boolean(a), Object::Boolean(b)) => a == b,
        (Object::Integer(a), Object::Integer(b)) => a == b,
        (Object::Real(a), Object::Real(b)) => a == b,
        (Object::Name(a), Object::Name(b)) => a == b,
        (Object::String(a, _), Object::String(b, _)) => a == b,
        (Object::Array(a), Object::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| objects_equivalent(x, y))
        }
        (Object::Dictionary(a), Object::Dictionary(b)) => dictionaries_equivalent(a, b),
        (Object::Stream(a), Object::Stream(b)) => {
            a.content == b.content && dictionaries_equivalent(&a.dict, &b.dict)
        }
        (Object::Reference(a), Object::Reference(b)) => a == b,
        _ => false,
    }
}

fn dictionaries_equivalent(left: &Dictionary, right: &Dictionary) -> bool {
    left.len() == right.len()
        && left.iter().all(|(key, value)| {
            matches!(right.get(key), Ok(other) if objects_equivalent(value, other))
        })
}

/// Rewrite `Reference(duplicate)` to `Reference(canonical)` inside every
/// object (and the trailer), then delete the duplicate objects outright —
/// unlike the earlier stub approach (an indirect object whose body is a
/// reference), in-edge rewriting leaves only spec-clean objects behind.
fn dedupe_apply_replacements(
    document: &mut Document,
    replacements: &HashMap<ObjectId, ObjectId>,
) {
    fn rewrite_object(object: &mut Object, mapping: &HashMap<ObjectId, ObjectId>) {
        match object {
            Object::Array(items) => {
                for item in items {
                    rewrite_object(item, mapping);
                }
            }
            Object::Dictionary(dict) => {
                for (_key, value) in &mut *dict {
                    rewrite_object(value, mapping);
                }
            }
            Object::Stream(stream) => {
                for (_key, value) in &mut stream.dict {
                    rewrite_object(value, mapping);
                }
            }
            Object::Reference(reference) => {
                if let Some(&canonical) = mapping.get(reference) {
                    *reference = canonical;
                }
            }
            _ => {}
        }
    }

    for object in document.objects.values_mut() {
        rewrite_object(object, replacements);
    }
    for (_key, value) in &mut document.trailer {
        rewrite_object(value, replacements);
    }
    for duplicate_id in replacements.keys() {
        document.objects.remove(duplicate_id);
    }
}

/// Merge fully identical stream objects — images, content streams, font
/// programs, form XObjects — into one canonical object each, rewriting every
/// incoming reference. Fully lossless: dictionaries must match key-for-key
/// (references only merge when they already point at the same object) and the
/// content bytes must be identical. Returns `(images_merged, other_merged)`.
fn dedupe_identical_streams(document: &mut Document) -> (u32, u32) {
    let stream_ids: Vec<ObjectId> = document
        .objects
        .iter()
        .filter_map(|(&id, object)| matches!(object, Object::Stream(_)).then_some(id))
        .collect();

    let mut first_by_hash: HashMap<u64, ObjectId> = HashMap::new();
    let mut replacements: HashMap<ObjectId, ObjectId> = HashMap::new();
    let mut images_merged = 0u32;
    let mut other_merged = 0u32;

    for id in stream_ids {
        // Already rewritten away as a duplicate of an earlier object.
        if replacements.contains_key(&id) {
            continue;
        }
        let Some(Object::Stream(stream)) = document.objects.get(&id) else {
            continue;
        };

        let hash = stream_fingerprint(stream);
        match first_by_hash.get(&hash) {
            Some(&first_id) => {
                // Verify exact equality — a hash alone must never merge streams.
                let identical = matches!(
                    (document.objects.get(&first_id), document.objects.get(&id)),
                    (Some(first), Some(dup)) if objects_equivalent(first, dup)
                );
                if identical {
                    replacements.insert(id, first_id);
                    if is_image_stream(stream) {
                        images_merged += 1;
                    } else {
                        other_merged += 1;
                    }
                }
            }
            None => {
                first_by_hash.insert(hash, id);
            }
        }
    }

    if !replacements.is_empty() {
        dedupe_apply_replacements(document, &replacements);
    }

    (images_merged, other_merged)
}

// ---------------------------------------------------------------------------
// Image stream batch optimization
// ---------------------------------------------------------------------------

/// Extract image streams from the document, optimize them (possibly in
/// parallel), and write the results back.
fn optimize_image_streams<F>(
    document: &mut Document,
    preparation: &DocumentPreparation,
    settings: &CompressionSettings,
    runtime: CompressionRuntime<'_>,
    stats: &mut CompressionStats,
    report_progress: &mut F,
    last_reported_percent: &mut f32,
) -> Result<(), AppError>
where
    F: FnMut(ProgressUpdate),
{
    let image_tasks = take_image_tasks(
        document,
        &preparation.image_object_ids,
        &preparation.shared_smask_ids,
        &preparation.color_space_by_image,
    );
    if image_tasks.is_empty() {
        report_progress(ProgressUpdate::new(
            "compressing",
            IMAGE_OPTIMIZATION_PROGRESS_END,
        ));
        return Ok(());
    }

    let task_count = image_tasks.len();
    let max_bitmap_estimate = image_tasks
        .iter()
        .map(|task| estimated_decoded_bitmap_bytes(&task.stream))
        .max()
        .unwrap_or(0);
    let worker_count = image_worker_count(task_count, max_bitmap_estimate);
    let skip_policy = SkipPolicy::for_document(
        task_count,
        settings.grayscale || settings.bilevel_codec.uses_ccitt(),
    );

    // --- Serial path (1 worker) ---
    if worker_count <= 1 {
        for (index, task) in image_tasks.into_iter().enumerate() {
            ensure_not_cancelled(runtime.cancel_flag, runtime.task_id)?;
            let ImageTask {
                object_id,
                stream,
                smask,
                color_space,
                ..
            } = task;
            let optimization = optimize_image_stream(
                &stream,
                smask.as_ref().map(|(_, smask)| smask),
                settings,
                runtime.cancel_flag,
                runtime.task_id,
                skip_policy,
                None,
                color_space.as_ref(),
            )?;
            apply_image_optimization(document, object_id, stream, smask, optimization, stats);
            report_progress_if_needed(
                report_progress,
                last_reported_percent,
                OBJECT_SCAN_PROGRESS_END,
                IMAGE_OPTIMIZATION_PROGRESS_END,
                index + 1,
                task_count,
            );
        }
        return Ok(());
    }

    // --- Parallel path: the shared scoped pool (see workers.rs) ---
    let mut completed = 0usize;
    run_worker_pool(
        image_tasks,
        worker_count,
        worker_count * CHANNEL_BUFFER_MULTIPLIER,
        runtime.cancel_flag,
        runtime.task_id,
        |task: ImageTask| {
            let ImageTask {
                object_id,
                stream,
                smask,
                color_space,
                ..
            } = task;
            let result = optimize_image_stream(
                &stream,
                smask.as_ref().map(|(_, smask)| smask),
                settings,
                runtime.cancel_flag,
                runtime.task_id,
                skip_policy,
                None,
                color_space.as_ref(),
            );
            // The borrowed originals travel back with the outcome so
            // the main thread can restore them on skip.
            ImageTaskOutcome {
                object_id,
                result,
                stream,
                smask,
            }
        },
        |outcome: ImageTaskOutcome| {
            let optimization = outcome.result?;
            apply_image_optimization(
                document,
                outcome.object_id,
                outcome.stream,
                outcome.smask,
                optimization,
                stats,
            );
            completed += 1;
            report_progress_if_needed(
                report_progress,
                last_reported_percent,
                OBJECT_SCAN_PROGRESS_END,
                IMAGE_OPTIMIZATION_PROGRESS_END,
                completed,
                task_count,
            );
            Ok(())
        },
    )?;

    Ok(())
}

/// Remove image streams from the document and return them as tasks, sorted
/// largest-first so the worker pool starts with the heaviest work and avoids
/// the "straggler" problem.
///
/// The `/SMask` stream is *moved* out of the document (not cloned): workers
/// used to hold a second copy of every transparency mask, doubling the memory
/// for scan-heavy files. Masks shared by several images are cloned instead —
/// every sharer must keep a valid target. The originals travel back with the
/// outcome and are restored verbatim on skip.
pub(super) fn take_image_tasks(
    document: &mut Document,
    image_object_ids: &[ObjectId],
    shared_smask_ids: &HashSet<ObjectId>,
    color_spaces: &HashMap<ObjectId, super::colorspace::ImageColorSpaceInfo>,
) -> Vec<ImageTask> {
    let mut tasks = Vec::with_capacity(image_object_ids.len());

    for object_id in image_object_ids.iter().copied() {
        let Some(object) = document.objects.remove(&object_id) else {
            continue;
        };
        match object {
            Object::Stream(stream) => {
                let stream_size = stream.content.len();
                // Resolve /SMask here — workers have no document access.
                let smask = stream
                    .dict
                    .get(b"SMask")
                    .ok()
                    .and_then(|entry| entry.as_reference().ok())
                    .and_then(|smask_id| {
                        if shared_smask_ids.contains(&smask_id) {
                            document
                                .objects
                                .get(&smask_id)
                                .and_then(|object| match object {
                                    Object::Stream(smask_stream) => {
                                        Some((smask_id, smask_stream.clone()))
                                    }
                                    _ => None,
                                })
                        } else {
                            match document.objects.remove(&smask_id) {
                                Some(Object::Stream(smask_stream)) => {
                                    Some((smask_id, smask_stream))
                                }
                                Some(other) => {
                                    // Not a stream after all — put it back untouched.
                                    document.objects.insert(smask_id, other);
                                    None
                                }
                                None => None,
                            }
                        }
                    });
                tasks.push(ImageTask {
                    object_id,
                    stream,
                    smask,
                    color_space: color_spaces.get(&object_id).cloned(),
                    stream_size,
                });
            }
            other => {
                document.objects.insert(object_id, other);
            }
        }
    }

    // Largest first → heavy tasks start immediately, reducing tail latency.
    tasks.sort_unstable_by_key(|task| std::cmp::Reverse(task.stream_size));
    tasks
}

/// Write an optimization result back into the document. `stream`/`smask` are
/// the moved-out originals: restored verbatim on skip, consumed (dropped) on
/// recompression.
fn apply_image_optimization(
    document: &mut Document,
    object_id: ObjectId,
    stream: Stream,
    smask: Option<(ObjectId, Stream)>,
    optimization: ImageOptimization,
    stats: &mut CompressionStats,
) {
    match optimization {
        ImageOptimization::Recompressed {
            stream: mut rebuilt,
            smask: rebuilt_smask,
        } => {
            if rebuilt.dict.get(b"Filter").is_ok_and(|filter| {
                matches!(filter, Object::Name(name) if name.as_slice() == b"CCITTFaxDecode")
            }) {
                stats.images_bilevel_encoded += 1;
            }
            if let Some(smask_stream) = rebuilt_smask {
                let smask_id = document.add_object(smask_stream);
                rebuilt.dict.set("SMask", Object::Reference(smask_id));
            }
            document.objects.insert(object_id, Object::Stream(rebuilt));
            stats.images_recompressed += 1;
        }
        ImageOptimization::Skipped { reason } => {
            document.objects.insert(object_id, Object::Stream(stream));
            if let Some((smask_id, smask_stream)) = smask {
                document
                    .objects
                    .insert(smask_id, Object::Stream(smask_stream));
            }
            record_image_skip(stats, object_id, reason);
        }
    }
}

pub(super) fn record_image_skip(stats: &mut CompressionStats, object_id: ObjectId, reason: String) {
    stats.images_skipped += 1;

    if stats.image_skip_notices >= MAX_IMAGE_SKIP_NOTICES {
        stats.suppressed_skip_notices += 1;
        return;
    }

    stats.image_skip_notices += 1;
    stats.notices.push(
        BackendNotice::new(
            "compress.warning.imageSkipped",
            "warning",
            format!("Skipped image object {:?}: {reason}", object_id),
        )
        .with_value("objectId", format!("{:?}", object_id))
        .with_value("reason", reason),
    );
}

// ---------------------------------------------------------------------------
// Non-image stream compression
// ---------------------------------------------------------------------------

/// Minimum number of eligible streams before parallel compression pays for
/// its thread coordination overhead.
const MIN_PARALLEL_STREAM_OBJECTS: usize = 8;

/// Hard cap on stream-compression threads; each holds one stream at a time
/// and deflate working memory is small, so this stays generous.
const MAX_STREAM_WORKERS: usize = 8;

/// Eligibility check: uncompressed, compressible, and large enough for
/// deflate to plausibly win. Runs on the scan thread per object.
fn stream_is_compressible(stream: &Stream) -> bool {
    !stream.is_compressed()
        && stream.allows_compression
        && stream.content.len() >= MIN_COMPRESSIBLE_STREAM_BYTES
}

/// Deflate-compress a batch of eligible non-image streams in parallel.
/// Streams are moved out, compressed in per-thread chunks (no locking), and
/// moved back — failures leave a stream untouched, exactly as before.
fn compress_non_image_streams(
    document: &mut Document,
    candidate_ids: &[ObjectId],
    cancel_flag: &Arc<AtomicBool>,
    task_id: &str,
) -> Result<usize, AppError> {
    // Move the candidates out so worker chunks can own their slice.
    let mut taken: Vec<(ObjectId, Stream)> = candidate_ids
        .iter()
        .filter_map(|&object_id| match document.objects.remove(&object_id) {
            Some(Object::Stream(stream)) => Some((object_id, stream)),
            Some(other) => {
                document.objects.insert(object_id, other);
                None
            }
            None => None,
        })
        .collect();

    let worker_count = stream_worker_count(taken.len());
    let compressed = if worker_count <= 1 {
        let mut compressed = 0;
        for (_, stream) in taken.iter_mut() {
            if cancel_flag.load(Ordering::Relaxed) {
                break;
            }
            if stream.compress().is_ok() {
                compressed += 1;
            }
        }
        compressed
    } else {
        let chunk_size = taken.len().div_ceil(worker_count);
        thread::scope(|scope| {
            let mut handles = Vec::with_capacity(worker_count);
            for chunk in taken.chunks_mut(chunk_size) {
                let cancel_flag = &*cancel_flag;
                handles.push(scope.spawn(move || {
                    let mut compressed = 0;
                    for (_, stream) in chunk.iter_mut() {
                        if cancel_flag.load(Ordering::Relaxed) {
                            break;
                        }
                        if stream.compress().is_ok() {
                            compressed += 1;
                        }
                    }
                    compressed
                }));
            }
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap_or(0))
                .sum()
        })
    };

    // Restore every stream, compressed or not.
    for (object_id, stream) in taken {
        document.objects.insert(object_id, Object::Stream(stream));
    }

    ensure_not_cancelled(cancel_flag, task_id)?;
    Ok(compressed)
}

fn stream_worker_count(candidate_count: usize) -> usize {
    if candidate_count < MIN_PARALLEL_STREAM_OBJECTS {
        return 1;
    }
    let available = thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);
    available
        .saturating_sub(1)
        .clamp(1, MAX_STREAM_WORKERS)
        .min(candidate_count)
}

// ---------------------------------------------------------------------------
// PDF dictionary helpers
// ---------------------------------------------------------------------------

fn is_image_stream(stream: &Stream) -> bool {
    // Byte-wise comparison — allocation-free, unlike optional_name, because
    // this runs against every stream in the document.
    matches!(stream.dict.get(b"Subtype"), Ok(Object::Name(name)) if name.as_slice() == b"Image")
}

// ---------------------------------------------------------------------------
// Metadata removal
// ---------------------------------------------------------------------------

fn remove_metadata(document: &mut Document) -> bool {
    let mut removed = false;

    if document.trailer.remove(b"Info").is_some() {
        removed = true;
    }

    let root_ref = document
        .trailer
        .get(b"Root")
        .ok()
        .and_then(|o| o.as_reference().ok());

    if let Some(root_ref) = root_ref {
        let meta_ref = document
            .objects
            .get(&root_ref)
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"Metadata").ok())
            .and_then(|o| o.as_reference().ok());

        if let Some(meta_ref) = meta_ref {
            document.objects.remove(&meta_ref);
            removed = true;
        }

        if let Some(Object::Dictionary(root_dict)) = document.objects.get_mut(&root_ref) {
            if root_dict.remove(b"Metadata").is_some() {
                removed = true;
            }
        }
    }

    removed
}

// ---------------------------------------------------------------------------
// Worker pool sizing
// ---------------------------------------------------------------------------

/// Soft budget for concurrently-held decoded bitmaps across the image worker
/// pool. The pool used to run a fixed 8 workers: eight concurrent decodes of
/// large scans (an 8000×8000 RGB page decodes to ~192 MB) can transiently
/// hold multiple GiB. Workers are now additionally bounded by this budget
/// divided by the largest single decoded-bitmap estimate.
const IMAGE_DECODE_MEMORY_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

/// Estimate the decoded size of a stream's bitmap. Pixel-count based (3 bytes
/// per pixel worst case); falls back to a ~12:1 JPEG compression heuristic
/// when the dictionary carries no usable dimensions.
pub(crate) fn estimated_decoded_bitmap_bytes(stream: &Stream) -> u64 {
    match pixel_count(stream) {
        Some(pixels) => pixels.saturating_mul(3),
        None => (stream.content.len() as u64).saturating_mul(12),
    }
}

/// Decide how many worker threads to use for image optimization.
/// Uses all available cores minus one (for the main thread), capped at
/// `MAX_IMAGE_WORKERS` and by the decode memory budget. Falls back to serial
/// for small task counts.
pub(crate) fn image_worker_count(task_count: usize, max_estimated_bitmap_bytes: u64) -> usize {
    if task_count < MIN_PARALLEL_IMAGE_OBJECTS {
        return 1;
    }

    let available = thread::available_parallelism()
        .map(|v| v.get())
        .unwrap_or(1);
    if available <= 1 {
        return 1;
    }

    let usable = available.saturating_sub(1).max(1);
    // A missing estimate (`checked_div` → None) or a bitmap larger than the
    // whole budget still allows one worker — the pool never goes serial here.
    let memory_cap = (IMAGE_DECODE_MEMORY_BUDGET_BYTES
        .checked_div(max_estimated_bitmap_bytes)
        .unwrap_or(0)
        .max(1)) as usize;
    task_count
        .min(usable)
        .min(MAX_IMAGE_WORKERS)
        .min(memory_cap)
}

// ---------------------------------------------------------------------------
// Progress reporting
// ---------------------------------------------------------------------------

/// Report progress only when the percentage advanced by ≥ 3 points or the
/// batch is complete. Avoids flooding the IPC channel with tiny updates.
fn report_progress_if_needed<F>(
    report_progress: &mut F,
    last_reported_percent: &mut f32,
    start_percent: f32,
    end_percent: f32,
    completed: usize,
    total: usize,
) where
    F: FnMut(ProgressUpdate),
{
    let total = total.max(1);
    let ratio = completed as f32 / total as f32;
    let next = start_percent + (end_percent - start_percent) * ratio;

    if next - *last_reported_percent >= 3.0 || completed >= total {
        *last_reported_percent = next;
        report_progress(ProgressUpdate::new("compressing", next.min(end_percent)));
    }
}

// ---------------------------------------------------------------------------
// File path helpers
// ---------------------------------------------------------------------------

/// Pick the output path for `input_path` according to `settings.output_dir`,
/// with the `__optimized-<preset>` naming and collision suffixes. Shared with
/// the target-size search, which materializes next to the source file.
pub(crate) fn build_output_path(
    input_path: &Path,
    settings: &CompressionSettings,
) -> Result<PathBuf, AppError> {
    let parent = if let Some(output_dir) = &settings.output_dir {
        let p = PathBuf::from(output_dir);
        if !p.exists() {
            return Err(AppError::PdfBuild(format!(
                "Output directory does not exist: {output_dir}"
            )));
        }
        p
    } else {
        input_path
            .parent()
            .ok_or_else(|| {
                AppError::PdfBuild(
                    "Could not determine the source directory for the output PDF.".into(),
                )
            })?
            .to_path_buf()
    };

    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| AppError::PdfBuild("Could not read the source filename.".into()))?;

    let base_name = format!("{stem}__optimized-{}", settings.preset.as_label());

    for attempt in 0..100u16 {
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("-{attempt}")
        };

        let candidate = parent.join(format!("{base_name}{suffix}.pdf"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(AppError::PdfBuild(
        "Could not find a free output filename after many attempts.".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_count_stays_serial_below_parallel_threshold() {
        assert_eq!(image_worker_count(0, 0), 1);
        assert_eq!(image_worker_count(1, 0), 1);
        assert_eq!(image_worker_count(MIN_PARALLEL_IMAGE_OBJECTS - 1, 0), 1);
    }

    #[test]
    fn worker_count_is_bounded_by_tasks_and_cap() {
        for count in [3usize, 16, 512] {
            let workers = image_worker_count(count, 0);
            assert!(workers >= 1, "workers must be at least 1");
            assert!(workers <= count, "workers must not exceed the task count");
            assert!(workers <= MAX_IMAGE_WORKERS, "workers must respect the cap");
        }
    }

    #[test]
    fn worker_count_shrinks_under_memory_pressure() {
        // A 192 MB worst-case bitmap (8000×8000 RGB) caps the pool at
        // budget / bitmap-bytes workers regardless of core count.
        let large_bitmap: u64 = 192 * 1024 * 1024;
        let workers = image_worker_count(64, large_bitmap);
        assert!(workers >= 1);
        assert!(
            workers <= (IMAGE_DECODE_MEMORY_BUDGET_BYTES / large_bitmap) as usize,
            "decode memory budget must bound the worker count"
        );
    }

    #[test]
    fn input_size_guard_rejects_oversized_files() {
        assert!(ensure_input_size_supported(1024).is_ok());
        assert!(ensure_input_size_supported(3 * 1024 * 1024 * 1024).is_err());
    }
}
