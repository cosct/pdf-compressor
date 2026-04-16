//! Object-level PDF compression engine — image recompression, stream compression, metadata removal.
//! 对象级 PDF 压缩引擎 — 图片重压缩、流压缩、元数据移除。
//!
//! Walks every PDF object, classifies streams, optimizes images (possibly in parallel
//! with a largest-first scheduling strategy), compresses eligible non-image streams,
//! and optionally strips document metadata. Supports cancellation via an atomic flag.
//! 遍历每个 PDF 对象，分类流，优化图片（可能通过最大优先调度策略并行处理），
//! 压缩符合条件的非图片流，可选移除文档元数据。支持通过原子标志取消。

use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Instant,
};

use image::{codecs::jpeg::JpegEncoder, imageops::FilterType, DynamicImage, GenericImageView};
use lopdf::{Document, Object, ObjectId, Stream};

use crate::{
    error::AppError,
    models::{BackendNotice, CompressionResponse, ProgressUpdate},
};

use super::settings::CompressionSettings;
use super::optional_integer;

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

/// JPEG streams smaller than this are skipped outright — the decode+encode
/// round-trip cost exceeds any realistic savings on tiny streams.
const TINY_JPEG_STREAM_BYTES: usize = 6 * 1024;

/// Streams below this byte count are considered "small". For small JPEGs that
/// are already within the target dimensions, recompression is skipped because
/// the potential savings are negligible.
const SMALL_IMAGE_STREAM_BYTES: usize = 64 * 1024;

/// Images with fewer total pixels than this are skipped entirely.
/// Recompressing a 100×100 icon yields almost no savings.
const TRIVIAL_PIXEL_COUNT: u64 = 10_000;

/// For small JPEG images that are already at or below the target edge, skip
/// if pixel count is under this threshold (roughly 500×500).
const SMALL_JPEG_PIXEL_COUNT: u64 = 250_000;

/// Only resize when the source edge exceeds the target by at least this factor.
/// Prevents pointless resize operations for near-target images.
const RESIZE_EDGE_TOLERANCE: f32 = 1.08;

/// Above this pixel count, use a fast two-pass resize: first a cheap Nearest
/// downsample to ~2× the target, then CatmullRom for the final pass.
/// Lowered from 8M to 4M to trigger two-pass earlier and save CPU on moderately
/// large images.
const TWO_PASS_RESIZE_PIXEL_THRESHOLD: u64 = 4_000_000;

/// Non-image streams shorter than this are too small for compression to help.
/// Lowered from 128 to 64 bytes — deflate can still win on repetitive short streams.
const MIN_COMPRESSIBLE_STREAM_BYTES: usize = 64;

/// Maximum number of per-image skip notices included in the response.
/// Additional skips are counted but not individually reported.
const MAX_IMAGE_SKIP_NOTICES: usize = 6;

/// Worker channel buffer multiplier relative to worker count.
/// Larger buffer reduces blocking on the producer side.
const CHANNEL_BUFFER_MULTIPLIER: usize = 4;

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct CompressionStats {
    images_recompressed: usize,
    images_skipped: usize,
    streams_compressed: usize,
    metadata_removed: bool,
    notices: Vec<BackendNotice>,
    image_skip_notices: usize,
    suppressed_skip_notices: usize,
}

#[derive(Debug)]
struct ImageTask {
    object_id: ObjectId,
    stream: Stream,
    /// Cached stream byte length — avoids re-reading during scheduling.
    stream_size: usize,
}

#[derive(Debug)]
struct ImageTaskOutcome {
    object_id: ObjectId,
    result: Result<ImageOptimization, AppError>,
}

#[derive(Debug, Clone, Copy, Default)]
struct StreamFilterInfo {
    has_jpeg: bool,
    has_flate: bool,
    has_unsupported_filter: bool,
}

#[derive(Clone, Copy)]
struct CompressionRuntime<'a> {
    cancel_flag: &'a Arc<AtomicBool>,
    task_id: &'a str,
}

#[derive(Debug)]
enum ImageOptimization {
    Recompressed(Stream),
    Skipped { stream: Stream, reason: String },
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Compress a PDF file according to the given settings, reporting progress via
/// the provided callback. Returns detailed statistics on what was optimized.
pub fn compress_pdf_with_progress<F>(
    path: &str,
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
    let output_path = build_output_path(&input_path, &settings)?;

    // --- Load the PDF document ---
    ensure_not_cancelled(&cancel_flag, path)?;
    let mut document = Document::load(&input_path)
        .map_err(|e| AppError::PdfBuild(format!("Failed to load PDF: {e}")))?;
    let mut stats = CompressionStats::default();

    report_progress(ProgressUpdate::new("compressing", OBJECT_SCAN_PROGRESS_START));

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

    // --- Cleanup & write output ---
    document.prune_objects();
    document.renumber_objects();

    let mut output_file = fs::File::create(&output_path)?;
    document
        .save_modern(&mut output_file)
        .map_err(|e| AppError::PdfBuild(format!("Failed to save optimized PDF: {e}")))?;

    // --- Compute result metrics ---
    let compressed_size_bytes = fs::metadata(&output_path)?.len();
    let saved_bytes = original_size_bytes.saturating_sub(compressed_size_bytes);
    let savings_percent = if original_size_bytes == 0 {
        0.0
    } else {
        (saved_bytes as f32 / original_size_bytes as f32) * 100.0
    };

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
    if !stats.metadata_removed {
        stats.notices.push(BackendNotice::new(
            "compress.note.metadataKept",
            "neutral",
            "Document metadata stayed in place because metadata cleanup was disabled or unavailable.",
        ));
    }

    report_progress(
        ProgressUpdate::new("done", 100.0).with_message(BackendNotice::new(
            "compress.progress.done",
            "success",
            "Compression finished.",
        )),
    );

    Ok(CompressionResponse {
        output_path: output_path.to_string_lossy().to_string(),
        original_size_bytes,
        compressed_size_bytes,
        saved_bytes,
        savings_percent,
        elapsed_ms: started_at.elapsed().as_millis(),
        images_recompressed: stats.images_recompressed,
        images_skipped: stats.images_skipped,
        streams_compressed: stats.streams_compressed,
        metadata_removed: stats.metadata_removed,
        output_was_smaller: compressed_size_bytes < original_size_bytes,
        notices: stats.notices,
    })
}

// ---------------------------------------------------------------------------
// Document-level optimization
// ---------------------------------------------------------------------------

/// Walk every object in the document exactly once:
/// - Collect image stream IDs for batch optimization.
/// - Compress eligible non-image streams inline.
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
    let object_ids: Vec<ObjectId> = document.objects.keys().copied().collect();
    let total_objects = object_ids.len().max(1);
    let mut image_object_ids = Vec::new();
    let mut last_reported_percent = OBJECT_SCAN_PROGRESS_START;

    // --- Single-pass scan: classify each object ---
    for (index, object_id) in object_ids.into_iter().enumerate() {
        ensure_not_cancelled(runtime.cancel_flag, runtime.task_id)?;
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
        } else if settings.compress_streams && compress_non_image_stream(stream) {
            stats.streams_compressed += 1;
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

    // --- Batch image optimization (possibly parallel) ---
    if settings.optimize_images && !image_object_ids.is_empty() {
        optimize_image_streams(
            document,
            &image_object_ids,
            settings,
            runtime,
            stats,
            report_progress,
            &mut last_reported_percent,
        )?;
    } else {
        report_progress(ProgressUpdate::new("compressing", IMAGE_OPTIMIZATION_PROGRESS_END));
    }

    // --- Metadata removal ---
    if settings.strip_metadata {
        stats.metadata_removed = remove_metadata(document);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Image stream batch optimization
// ---------------------------------------------------------------------------

/// Extract image streams from the document, optimize them (possibly in
/// parallel), and write the results back.
fn optimize_image_streams<F>(
    document: &mut Document,
    image_object_ids: &[ObjectId],
    settings: &CompressionSettings,
    runtime: CompressionRuntime<'_>,
    stats: &mut CompressionStats,
    report_progress: &mut F,
    last_reported_percent: &mut f32,
) -> Result<(), AppError>
where
    F: FnMut(ProgressUpdate),
{
    let image_tasks = take_image_tasks(document, image_object_ids);
    if image_tasks.is_empty() {
        report_progress(ProgressUpdate::new("compressing", IMAGE_OPTIMIZATION_PROGRESS_END));
        return Ok(());
    }

    let task_count = image_tasks.len();
    let worker_count = image_worker_count(task_count);

    // --- Serial path (1 worker) ---
    if worker_count <= 1 {
        for (index, task) in image_tasks.into_iter().enumerate() {
            ensure_not_cancelled(runtime.cancel_flag, runtime.task_id)?;
            let optimization = optimize_image_stream(
                task.stream,
                settings,
                runtime.cancel_flag,
                runtime.task_id,
            )?;
            apply_image_optimization(document, task.object_id, optimization, stats);
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

    // --- Parallel path ---
    thread::scope(|scope| -> Result<(), AppError> {
        let cancel_flag = Arc::clone(runtime.cancel_flag);
        // Larger buffer to reduce producer blocking.
        let (task_tx, task_rx) =
            mpsc::sync_channel::<ImageTask>(worker_count * CHANNEL_BUFFER_MULTIPLIER);
        let task_rx = Arc::new(Mutex::new(task_rx));
        let (result_tx, result_rx) = mpsc::channel::<ImageTaskOutcome>();

        // Spawn worker threads.
        for _ in 0..worker_count {
            let rx = Arc::clone(&task_rx);
            let tx = result_tx.clone();
            let worker_settings = settings.clone();
            let worker_cancel_flag = Arc::clone(&cancel_flag);
            let worker_task_id = runtime.task_id.to_string();

            scope.spawn(move || {
                loop {
                    if worker_cancel_flag.load(Ordering::Relaxed) {
                        return;
                    }

                    let task = match rx.lock() {
                        Ok(guard) => guard.recv(),
                        Err(_) => return,
                    };
                    let Ok(task) = task else { return };

                    let result = optimize_image_stream(
                        task.stream,
                        &worker_settings,
                        &worker_cancel_flag,
                        &worker_task_id,
                    );
                    if tx
                        .send(ImageTaskOutcome {
                            object_id: task.object_id,
                            result,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            });
        }

        // Drop spare sender so result_rx closes when all workers finish.
        drop(result_tx);

        // Feed tasks into the channel.
        for task in image_tasks {
            ensure_not_cancelled(&cancel_flag, runtime.task_id)?;
            task_tx.send(task).map_err(|_| {
                AppError::PdfBuild("Failed to schedule an image optimization task.".to_string())
            })?;
        }
        drop(task_tx);

        // Collect results.
        for completed in 0..task_count {
            ensure_not_cancelled(&cancel_flag, runtime.task_id)?;
            let outcome = result_rx.recv().map_err(|_| {
                AppError::PdfBuild(
                    "An image optimization worker exited before returning its result.".to_string(),
                )
            })?;
            let optimization = outcome.result?;
            apply_image_optimization(document, outcome.object_id, optimization, stats);
            report_progress_if_needed(
                report_progress,
                last_reported_percent,
                OBJECT_SCAN_PROGRESS_END,
                IMAGE_OPTIMIZATION_PROGRESS_END,
                completed + 1,
                task_count,
            );
        }

        Ok(())
    })?;

    Ok(())
}

/// Remove image streams from the document and return them as tasks, sorted
/// largest-first so the worker pool starts with the heaviest work and avoids
/// the "straggler" problem.
fn take_image_tasks(document: &mut Document, image_object_ids: &[ObjectId]) -> Vec<ImageTask> {
    let mut tasks = Vec::with_capacity(image_object_ids.len());

    for object_id in image_object_ids.iter().copied() {
        let Some(object) = document.objects.remove(&object_id) else {
            continue;
        };
        match object {
            Object::Stream(stream) => {
                let stream_size = stream.content.len();
                tasks.push(ImageTask {
                    object_id,
                    stream,
                    stream_size,
                });
            }
            other => {
                document.objects.insert(object_id, other);
            }
        }
    }

    // Largest first → heavy tasks start immediately, reducing tail latency.
    tasks.sort_unstable_by(|a, b| b.stream_size.cmp(&a.stream_size));
    tasks
}

fn apply_image_optimization(
    document: &mut Document,
    object_id: ObjectId,
    optimization: ImageOptimization,
    stats: &mut CompressionStats,
) {
    match optimization {
        ImageOptimization::Recompressed(stream) => {
            document.objects.insert(object_id, Object::Stream(stream));
            stats.images_recompressed += 1;
        }
        ImageOptimization::Skipped { stream, reason } => {
            document.objects.insert(object_id, Object::Stream(stream));
            record_image_skip(stats, object_id, reason);
        }
    }
}

fn record_image_skip(stats: &mut CompressionStats, object_id: ObjectId, reason: String) {
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
// Per-image optimization
// ---------------------------------------------------------------------------

/// Decide whether to recompress a single image stream. Returns quickly for
/// images that cannot benefit from recompression (fast-path skips).
fn optimize_image_stream(
    mut stream: Stream,
    settings: &CompressionSettings,
    cancel_flag: &Arc<AtomicBool>,
    task_id: &str,
) -> Result<ImageOptimization, AppError> {
    ensure_not_cancelled(cancel_flag, task_id)?;
    // --- Skip: transparency / masks are not safely rewritable ---
    if has_mask(&stream) {
        return Ok(ImageOptimization::Skipped {
            stream,
            reason: "transparency or image masks are not rewritten yet".into(),
        });
    }

    let filter_info = stream_filter_info(&stream);

    // --- Skip: unsupported filters (JBIG2, JPX, CCITT, Crypt) ---
    if filter_info.has_unsupported_filter {
        return Ok(ImageOptimization::Skipped {
            stream,
            reason: "unsupported image filter for safe recompression".into(),
        });
    }

    // --- Fast skip: tiny JPEG streams ---
    if filter_info.has_jpeg && stream.content.len() <= TINY_JPEG_STREAM_BYTES {
        return Ok(ImageOptimization::Skipped {
            stream,
            reason: "JPEG stream too small to benefit from recompression".into(),
        });
    }

    // --- Fast skip: trivially small images by pixel count ---
    if let Some(pixels) = pixel_count(&stream) {
        if pixels <= TRIVIAL_PIXEL_COUNT {
            return Ok(ImageOptimization::Skipped {
                stream,
                reason: "image too small in pixel dimensions to benefit".into(),
            });
        }
    }

    let max_edge = u32::from(settings.max_image_size_px);

    // --- Fast JPEG header check: read dimensions without full decode ---
    // This is orders of magnitude faster than `image::load_from_memory`.
    if filter_info.has_jpeg {
        if let Some((w, h)) = jpeg_dimensions_from_header(&stream.content) {
            let longest = w.max(h);

            // Already within target AND stream is compact → skip.
            if longest <= max_edge && stream.content.len() <= SMALL_IMAGE_STREAM_BYTES {
                return Ok(ImageOptimization::Skipped {
                    stream,
                    reason: "JPEG already within target dimensions and stream size".into(),
                });
            }

            // Near target edge AND very small stream → skip for speed.
            let tolerance_edge = (max_edge as f32 * RESIZE_EDGE_TOLERANCE) as u32;
            if longest <= tolerance_edge && stream.content.len() <= SMALL_IMAGE_STREAM_BYTES / 2 {
                return Ok(ImageOptimization::Skipped {
                    stream,
                    reason: "JPEG near target dimensions with small stream".into(),
                });
            }
        }
    }

    // --- Dictionary-based dimension check (catches non-JPEG too) ---
    if let Some(edge) = longest_edge(&stream) {
        if filter_info.has_jpeg && edge <= max_edge && stream.content.len() <= SMALL_IMAGE_STREAM_BYTES
        {
            return Ok(ImageOptimization::Skipped {
                stream,
                reason: "already below target size and unlikely to shrink".into(),
            });
        }
    }

    // --- Additional heuristic skip checks ---
    if let Some(reason) = skip_recompression_reason(&stream, settings, filter_info) {
        return Ok(ImageOptimization::Skipped { stream, reason });
    }

    if let Some(reason) = raw_recompression_skip_reason(&stream, filter_info) {
        return Ok(ImageOptimization::Skipped { stream, reason });
    }

    let original_len = stream.content.len();

    // --- Decode the image ---
    ensure_not_cancelled(cancel_flag, task_id)?;
    let dynamic_image = if filter_info.has_jpeg {
        image::load_from_memory(&stream.content)
            .map_err(|e| AppError::PdfBuild(format!("Failed to decode JPEG image stream: {e}")))?
    } else {
        decode_raw_image_stream(&stream)?
    };

    // --- Resize if needed (two-pass for large images) ---
    ensure_not_cancelled(cancel_flag, task_id)?;
    let optimized = resize_if_needed_fast(dynamic_image, settings.max_image_size_px);
    let color_space_name = if optimized.color().has_color() {
        "DeviceRGB"
    } else {
        "DeviceGray"
    };

    // --- Encode as JPEG ---
    // Pre-allocate based on conservative compression ratio estimate.
    ensure_not_cancelled(cancel_flag, task_id)?;
    let estimated_output_size = (original_len as f32 * 0.65) as usize;
    let encoded =
        encode_dynamic_image_as_jpeg(&optimized, settings.image_quality, estimated_output_size)?;

    // --- Skip if the new encoding is not smaller AND we didn't resize ---
    if encoded.len() >= original_len
        && longest_edge(&stream).is_some_and(|edge| edge <= max_edge)
    {
        return Ok(ImageOptimization::Skipped {
            stream,
            reason: "existing image stream is already compact for the requested target".into(),
        });
    }

    // --- Write optimized stream back ---
    stream
        .dict
        .set("Filter", Object::Name(b"DCTDecode".to_vec()));
    stream.dict.remove(b"DecodeParms");
    stream
        .dict
        .set("Width", Object::Integer(optimized.width().into()));
    stream
        .dict
        .set("Height", Object::Integer(optimized.height().into()));
    stream.dict.set("BitsPerComponent", Object::Integer(8));
    stream.dict.set(
        "ColorSpace",
        Object::Name(color_space_name.as_bytes().to_vec()),
    );
    stream.set_content(encoded);

    Ok(ImageOptimization::Recompressed(stream))
}

fn ensure_not_cancelled(cancel_flag: &Arc<AtomicBool>, task_id: &str) -> Result<(), AppError> {
    if cancel_flag.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled(task_id.to_string()));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Skip heuristics
// ---------------------------------------------------------------------------

fn skip_recompression_reason(
    stream: &Stream,
    settings: &CompressionSettings,
    filter_info: StreamFilterInfo,
) -> Option<String> {
    let edge = longest_edge(stream)?;
    if edge > u32::from(settings.max_image_size_px) {
        return None;
    }

    if stream.content.len() <= SMALL_IMAGE_STREAM_BYTES {
        return Some("already below target and unlikely to shrink meaningfully".into());
    }

    let pixels = pixel_count(stream)?;
    if filter_info.has_jpeg && pixels <= SMALL_JPEG_PIXEL_COUNT {
        return Some("already near the requested target dimensions".into());
    }

    None
}

fn raw_recompression_skip_reason(stream: &Stream, filter_info: StreamFilterInfo) -> Option<String> {
    if filter_info.has_jpeg {
        return None;
    }

    let bits_per_component = optional_integer(stream, b"BitsPerComponent").unwrap_or(8);
    if bits_per_component != 8 {
        return Some(format!(
            "raw image uses unsupported bit depth for safe recompression: {bits_per_component}"
        ));
    }

    let color_space =
        optional_name(stream, b"ColorSpace").unwrap_or_else(|| "DeviceRGB".to_string());
    match color_space.as_str() {
        "DeviceGray" | "DeviceRGB" => None,
        other => Some(format!(
            "raw image uses unsupported color space for safe recompression: {other}"
        )),
    }
}

// ---------------------------------------------------------------------------
// Resize logic
// ---------------------------------------------------------------------------

/// Fast, two-pass resize strategy optimized for throughput:
///
/// 1. If the image is within the edge tolerance, return as-is (no work).
/// 2. For images above `TWO_PASS_RESIZE_PIXEL_THRESHOLD`:
///    - **Pass 1**: `Nearest` filter to ~2× the target (extremely fast, removes
///      the bulk of pixels).
///    - **Pass 2**: `CatmullRom` filter to exact target (good quality, but now
///      operating on a much smaller image).
/// 3. For smaller images: single-pass `CatmullRom` (fast enough at this size).
///
/// `CatmullRom` (bicubic) is ~2× faster than `Lanczos3` with nearly
/// indistinguishable quality for JPEG-bound output.
fn resize_if_needed_fast(image: DynamicImage, max_edge: u16) -> DynamicImage {
    let (width, height) = image.dimensions();
    let longest = width.max(height);
    let max_edge_u32 = u32::from(max_edge);

    // Within tolerance → no resize needed.
    let threshold = (max_edge_u32 as f32 * RESIZE_EDGE_TOLERANCE) as u32;
    if longest <= threshold {
        return image;
    }

    let scale = max_edge_u32 as f32 / longest as f32;
    let target_w = ((width as f32) * scale).round().max(1.0) as u32;
    let target_h = ((height as f32) * scale).round().max(1.0) as u32;

    let pixel_count = (width as u64) * (height as u64);

    if pixel_count > TWO_PASS_RESIZE_PIXEL_THRESHOLD {
        // Two-pass: bulk downsample with Nearest, then refine with CatmullRom.
        let inter_w = (target_w * 2).min(width);
        let inter_h = (target_h * 2).min(height);

        // Only bother with two-pass if intermediate is meaningfully smaller.
        if inter_w < width * 3 / 4 {
            let intermediate = image.resize(inter_w, inter_h, FilterType::Nearest);
            return intermediate.resize(target_w, target_h, FilterType::CatmullRom);
        }
    }

    // Single-pass CatmullRom — fast and high quality for moderate images.
    image.resize(target_w, target_h, FilterType::CatmullRom)
}

// ---------------------------------------------------------------------------
// Image decode / encode helpers
// ---------------------------------------------------------------------------

/// Decode a raw (non-JPEG) image stream using PDF dictionary metadata.
fn decode_raw_image_stream(stream: &Stream) -> Result<DynamicImage, AppError> {
    let width = required_integer(stream, b"Width")? as u32;
    let height = required_integer(stream, b"Height")? as u32;
    let bits_per_component = optional_integer(stream, b"BitsPerComponent").unwrap_or(8);
    let color_space =
        optional_name(stream, b"ColorSpace").unwrap_or_else(|| "DeviceRGB".to_string());

    if bits_per_component != 8 {
        return Err(AppError::PdfBuild(
            "Only 8-bit raw images are currently supported for recompression.".into(),
        ));
    }

    let decoded = stream
        .decompressed_content()
        .map_err(|e| AppError::PdfBuild(format!("Failed to decompress raw image stream: {e}")))?;

    match color_space.as_str() {
        "DeviceGray" => image::GrayImage::from_raw(width, height, decoded)
            .map(DynamicImage::ImageLuma8)
            .ok_or_else(|| {
                AppError::PdfBuild(
                    "Grayscale image bytes did not match the declared dimensions.".into(),
                )
            }),
        "DeviceRGB" => image::RgbImage::from_raw(width, height, decoded)
            .map(DynamicImage::ImageRgb8)
            .ok_or_else(|| {
                AppError::PdfBuild("RGB image bytes did not match the declared dimensions.".into())
            }),
        other => Err(AppError::PdfBuild(format!(
            "Unsupported color space for recompression: {other}"
        ))),
    }
}

/// Encode a `DynamicImage` as JPEG into a `Vec<u8>`.
fn encode_dynamic_image_as_jpeg(
    image: &DynamicImage,
    quality: u8,
    expected_capacity: usize,
) -> Result<Vec<u8>, AppError> {
    // At least 32 KB to avoid re-allocation on small images.
    let capacity = expected_capacity.max(32 * 1024);
    let mut cursor = Cursor::new(Vec::with_capacity(capacity));
    let mut encoder = JpegEncoder::new_with_quality(&mut cursor, quality);
    encoder.encode_image(image)?;
    Ok(cursor.into_inner())
}

/// Read JPEG dimensions from the SOF marker without decoding the full image.
/// Scans only the first 64 KB of the stream. Returns `(width, height)`.
fn jpeg_dimensions_from_header(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }

    let mut pos = 2;
    let scan_limit = data.len().min(65536);

    while pos + 4 < scan_limit {
        if data[pos] != 0xFF {
            pos += 1;
            continue;
        }

        let marker = data[pos + 1];

        // SOF markers: C0–CF except C4 (DHT) and CC (DAC).
        let is_sof = matches!(marker, 0xC0..=0xCF) && marker != 0xC4 && marker != 0xCC;

        if is_sof {
            if pos + 9 < data.len() {
                let height = u16::from_be_bytes([data[pos + 5], data[pos + 6]]) as u32;
                let width = u16::from_be_bytes([data[pos + 7], data[pos + 8]]) as u32;
                if width > 0 && height > 0 {
                    return Some((width, height));
                }
            }
            return None;
        }

        // Skip past this marker segment.
        if pos + 3 < data.len() {
            let segment_len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
            pos += 2 + segment_len;
        } else {
            break;
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Non-image stream compression
// ---------------------------------------------------------------------------

/// Try to deflate-compress a non-image stream. Returns `true` if compression
/// was applied.
fn compress_non_image_stream(stream: &mut Stream) -> bool {
    if stream.is_compressed() || !stream.allows_compression {
        return false;
    }

    // Skip streams too small for deflate to help.
    if stream.content.len() < MIN_COMPRESSIBLE_STREAM_BYTES {
        return false;
    }

    stream.compress().is_ok()
}

// ---------------------------------------------------------------------------
// PDF dictionary helpers
// ---------------------------------------------------------------------------

fn is_image_stream(stream: &Stream) -> bool {
    matches!(optional_name(stream, b"Subtype").as_deref(), Some("Image"))
}

fn has_mask(stream: &Stream) -> bool {
    stream.dict.get(b"SMask").is_ok() || stream.dict.get(b"Mask").is_ok()
}

fn stream_filter_info(stream: &Stream) -> StreamFilterInfo {
    let mut info = StreamFilterInfo::default();

    match stream.dict.get(b"Filter") {
        Ok(Object::Name(name)) => update_filter_info(name, &mut info),
        Ok(Object::Array(items)) => {
            for item in items {
                if let Object::Name(name) = item {
                    update_filter_info(name.as_slice(), &mut info);
                }
            }
        }
        _ => {}
    }

    info
}

fn update_filter_info(name: &[u8], info: &mut StreamFilterInfo) {
    match name {
        b"DCTDecode" => info.has_jpeg = true,
        b"FlateDecode" => info.has_flate = true,
        b"JPXDecode" | b"JBIG2Decode" | b"CCITTFaxDecode" | b"Crypt" => {
            info.has_unsupported_filter = true;
        }
        _ => {}
    }
}

fn longest_edge(stream: &Stream) -> Option<u32> {
    let w = optional_integer(stream, b"Width")?;
    let h = optional_integer(stream, b"Height")?;
    if w <= 0 || h <= 0 {
        return None;
    }
    Some(w.max(h) as u32)
}

fn pixel_count(stream: &Stream) -> Option<u64> {
    let w = optional_integer(stream, b"Width")?;
    let h = optional_integer(stream, b"Height")?;
    if w <= 0 || h <= 0 {
        return None;
    }
    Some((w as u64).saturating_mul(h as u64))
}

fn optional_name(stream: &Stream, key: &[u8]) -> Option<String> {
    match stream.dict.get(key) {
        Ok(Object::Name(name)) => Some(String::from_utf8_lossy(name).into_owned()),
        _ => None,
    }
}

fn required_integer(stream: &Stream, key: &[u8]) -> Result<i64, AppError> {
    optional_integer(stream, key).ok_or_else(|| {
        AppError::PdfBuild(format!(
            "Image stream missing required integer key: {}",
            String::from_utf8_lossy(key)
        ))
    })
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

/// Decide how many worker threads to use for image optimization.
/// Uses all available cores minus one (for the main thread), capped at
/// `MAX_IMAGE_WORKERS`. Falls back to serial for small task counts.
fn image_worker_count(image_count: usize) -> usize {
    if image_count < MIN_PARALLEL_IMAGE_OBJECTS {
        return 1;
    }

    let available = thread::available_parallelism()
        .map(|v| v.get())
        .unwrap_or(1);
    if available <= 1 {
        return 1;
    }

    let usable = available.saturating_sub(1).max(1);
    image_count.min(usable).min(MAX_IMAGE_WORKERS)
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

fn build_output_path(input_path: &Path, settings: &CompressionSettings) -> Result<PathBuf, AppError> {
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
                AppError::PdfBuild("Could not determine the source directory for the output PDF.".into())
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

fn validate_input_path(path: &str) -> Result<PathBuf, AppError> {
    let candidate = PathBuf::from(path);

    if !candidate.exists() {
        return Err(AppError::MissingInput(candidate));
    }

    let is_pdf = candidate
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"));

    if !is_pdf {
        return Err(AppError::InvalidPdfPath(candidate));
    }

    Ok(candidate)
}
