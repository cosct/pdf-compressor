//! Per-image codec logic — skip heuristics, decode, resize, JPEG re-encode,
//! and soft-mask (alpha) rewrites.
//! 单图编解码逻辑 — 跳过启发式、解码、缩放、JPEG 重编码与软蒙版（透明度）重写。

use std::sync::{atomic::AtomicBool, Arc};

use image::{imageops::FilterType, DynamicImage, GenericImageView};
#[cfg(feature = "ccitt")]
use lopdf::Dictionary;
use lopdf::{dictionary, Object, Stream};

use super::colorspace::{DecodeColorSpace, ImageColorSpaceInfo};
use super::ensure_not_cancelled;
use super::settings::CompressionSettings;
use super::optional_integer;
use crate::error::AppError;

/// JPEG streams smaller than this are skipped outright — the decode+encode
/// round-trip cost exceeds any realistic savings on tiny streams.
pub(super) const TINY_JPEG_STREAM_BYTES: usize = 6 * 1024;

/// Streams below this byte count are considered "small". For small JPEGs that
/// are already within the target dimensions, recompression is skipped because
/// the potential savings are negligible.
const SMALL_IMAGE_STREAM_BYTES: usize = 64 * 1024;

/// Documents with at least this many image objects are "image-heavy": the
/// per-image small-stream skip is lifted because aggregate savings across
/// hundreds of small scans justify the decode cost. The final
/// "only replace when smaller" check still guarantees no per-image bloat.
pub(super) const SMALL_SKIP_LIFT_IMAGE_OBJECTS: usize = 24;

/// Document-level skip policy — how aggressively already-compact images may
/// be skipped. Derived once per run and shared with the analyzer so the
/// savings estimate mirrors what the compressor will actually do.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SkipPolicy {
    /// Streams at or below this size that already fit the target dimensions
    /// are skipped. Drops to the tiny floor for image-heavy documents (many
    /// small scans add up) and when grayscale conversion was requested (the
    /// user asked for a conversion, not just shrinkage).
    pub small_stream_bytes: usize,
}

impl SkipPolicy {
    /// Policy for a document with `image_object_count` image objects. The
    /// small-stream skip is lifted when a conversion was requested (grayscale
    /// or CCITT G4 — the user asked for a conversion, not just shrinkage) or
    /// the document is image-heavy (many small scans add up).
    pub fn for_document(
        image_object_count: usize,
        conversion_requested: bool,
    ) -> Self {
        let lifted = conversion_requested || image_object_count >= SMALL_SKIP_LIFT_IMAGE_OBJECTS;
        Self {
            small_stream_bytes: if lifted {
                TINY_JPEG_STREAM_BYTES
            } else {
                SMALL_IMAGE_STREAM_BYTES
            },
        }
    }
}

/// Images with fewer total pixels than this are skipped entirely.
/// Recompressing a 100×100 icon yields almost no savings.
pub(super) const TRIVIAL_PIXEL_COUNT: u64 = 10_000;

// ---------------------------------------------------------------------------
// Per-image quality allocation (target-size search rounds)
// ---------------------------------------------------------------------------

/// Quality offset applied to low-detail (flat) planes during target-size
/// search rounds. Flat content survives low quality nearly unharmed, and its
/// bytes are better spent keeping detail-rich planes sharp.
const DETAIL_LOW_QUALITY_OFFSET: i16 = -12;

/// Quality offset applied to high-detail planes — funded by the savings from
/// the flat planes, roughly size-neutral per round.
const DETAIL_HIGH_QUALITY_OFFSET: i16 = 6;

/// Mean absolute luma-gradient below this score marks a plane "flat".
const DETAIL_LOW_SCORE: f32 = 8.0;

/// Mean absolute luma-gradient above this score marks a plane "detailed".
const DETAIL_HIGH_SCORE: f32 = 25.0;

/// Sample stride for the detail score — every 4th row and column keeps the
/// pass cheap on large planes while staying deterministic.
const DETAIL_SAMPLE_STRIDE: u32 = 4;

/// Mean absolute luma gradient over a strided sample of the plane. A cheap,
/// deterministic proxy for JPEG "detail": flat scans score near zero,
/// noisy photographs score high.
fn plane_detail_score(plane: &DynamicImage) -> f32 {
    use image::Pixel as _;

    let luma_at = |x: u32, y: u32| -> u32 {
        u32::from(plane.get_pixel(x, y).to_luma().0[0])
    };

    let (width, height) = plane.dimensions();
    if width < 2 || height < 2 {
        return 0.0;
    }

    let mut total: u32 = 0;
    let mut count: u32 = 0;
    let mut y = 0;
    while y < height - 1 {
        let mut x = 0;
        while x < width - 1 {
            let here = luma_at(x, y);
            total += (here as i32 - luma_at(x + 1, y) as i32).unsigned_abs();
            total += (here as i32 - luma_at(x, y + 1) as i32).unsigned_abs();
            count += 2;
            x += DETAIL_SAMPLE_STRIDE;
        }
        y += DETAIL_SAMPLE_STRIDE;
    }

    if count == 0 {
        0.0
    } else {
        total as f32 / count as f32
    }
}

/// Effective per-image quality for one target-size search round: detail-rich
/// planes get a boost, flat planes a penalty, so the round's byte budget is
/// spent where artifacts are visible. A pure function of the (deterministic)
/// post-resize plane, so probe estimates and materialized bytes stay in sync.
fn search_round_quality(base_quality: u8, plane: &DynamicImage) -> u8 {
    let score = plane_detail_score(plane);
    let offset = if score < DETAIL_LOW_SCORE {
        DETAIL_LOW_QUALITY_OFFSET
    } else if score > DETAIL_HIGH_SCORE {
        DETAIL_HIGH_QUALITY_OFFSET
    } else {
        0
    };
    (i16::from(base_quality) + offset).clamp(10, 100) as u8
}

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

/// Target-size search caches: a single cached plane larger than this is not
/// worth keeping across probe rounds (it would dominate the cache budget).
/// After a resize the shrunk plane is a few MB, so this rarely triggers.
const BITMAP_CACHE_SINGLE_MAX_BYTES: u64 = 96 * 1024 * 1024;

/// Luma values strictly inside this band count as "midtone" when deciding
/// whether a plane is effectively black-and-white; anything darker or lighter
/// counts as an extreme. JPEG noise around bilevel content sits at the
/// extremes, so only true midtones disqualify G4.
#[cfg(feature = "ccitt")]
const BILEVEL_EXTREME_BAND: u8 = 24;

/// A plane is treated as near-bilevel (a scan of text/line art) when at most
/// this fraction of pixels are midtone. Text antialiasing edges stay in the
/// low single digits of percent; photographs land far above.
#[cfg(feature = "ccitt")]
const NEAR_BILEVEL_MIDTONE_FRACTION: f64 = 0.05;

#[derive(Debug)]
// The two `Stream`s dominate the recompressed variant; boxing them would add
// an allocation per rewritten image for no real win — these move, they are
// not stored in bulk.
#[allow(clippy::large_enum_variant)]
pub(super) enum ImageOptimization {
    /// Recompressed stream plus, for images with transparency, the rebuilt
    /// soft-mask stream to attach as a new indirect object.
    Recompressed {
        stream: Stream,
        smask: Option<Stream>,
    },
    /// The original stream stays in place; the caller restores it (and its
    /// soft mask) from the originals it still owns.
    Skipped { reason: String },
}

/// Per-image caches that survive across target-size probe rounds. JPEG decode
/// results do not depend on the quality knob and the edge only ever shrinks,
/// so the decoded (and pre-shrunk) planes are computed once and reused — each
/// probe round then only re-encodes.
#[derive(Default)]
pub(super) struct ImageSearchCache {
    /// Decoded color plane, pre-shrunk to the first probed edge. Later,
    /// smaller-edge rounds resize down from it.
    bitmap: Option<DynamicImage>,
    /// Set once decoding failed, so later rounds skip the retry.
    undecodable_reason: Option<String>,
    /// Decoded alpha plane; outer `None` = not decoded yet, inner `None` =
    /// decoded once and found unsupported.
    smask_gray: Option<Option<image::GrayImage>>,
    /// Memoized (resize + flate) alpha stream keyed by target dimensions —
    /// quality-only rounds reproduce identical alpha bytes.
    smask_product: Option<((u32, u32), Stream)>,
    /// Memoized CCITT G4 bytes keyed by target dimensions — G4 has no quality
    /// knob, so quality-only probe rounds reproduce identical bytes.
    #[cfg(feature = "ccitt")]
    bilevel_product: Option<((u32, u32), Vec<u8>)>,
}

impl ImageSearchCache {
    pub(super) fn cached_bitmap_bytes(&self) -> Option<u64> {
        self.bitmap
            .as_ref()
            .map(|bitmap| bitmap.as_bytes().len() as u64)
    }

    /// Total cached bytes across every memoized plane and product — the
    /// accounting the document-level budget evicts against. Alpha planes are
    /// 1 byte per pixel; product entries hold their encoded stream bytes.
    pub(super) fn cached_bytes(&self) -> u64 {
        let bitmap = self
            .bitmap
            .as_ref()
            .map_or(0, |plane| plane.as_bytes().len() as u64);
        let smask_gray = self
            .smask_gray
            .as_ref()
            .map_or(0, |plane| {
                plane
                    .as_ref()
                    .map_or(0, |gray| gray.as_raw().len() as u64)
            });
        let smask_product = self
            .smask_product
            .as_ref()
            .map_or(0, |(_, stream)| stream.content.len() as u64);
        #[cfg(feature = "ccitt")]
        let bilevel_product = self
            .bilevel_product
            .as_ref()
            .map_or(0, |(_, bytes)| bytes.len() as u64);
        #[cfg(not(feature = "ccitt"))]
        let bilevel_product = 0;
        bitmap + smask_gray + smask_product + bilevel_product
    }

    pub(super) fn drop_bitmap(&mut self) {
        self.bitmap = None;
    }

    /// Second-stage eviction: drop the alpha and G4 products (quality-only
    /// round memoization) while keeping the sticky failure memo. Evicted
    /// entries re-derive their products on the next probe round.
    pub(super) fn drop_products(&mut self) {
        self.smask_gray = None;
        self.smask_product = None;
        #[cfg(feature = "ccitt")]
        {
            self.bilevel_product = None;
        }
    }

    fn store_bitmap(&mut self, plane: DynamicImage) {
        if plane.as_bytes().len() as u64 <= BITMAP_CACHE_SINGLE_MAX_BYTES {
            self.bitmap = Some(plane);
        }
    }
}

/// Codec classification of an image stream: `(is_jpeg, codec_supported)`.
/// Shared with the analyzer so the savings estimate can mirror what the
/// compressor would actually attempt.
pub(crate) fn image_codec_class(stream: &Stream) -> (bool, bool) {
    let info = stream_filter_info(stream);
    (info.has_jpeg, !info.has_unsupported_filter)
}

/// Would the compressor plausibly re-encode an image with these observed
/// properties at `target_edge` under `policy`? Mirrors the fast-path skip
/// heuristics of `optimize_image_stream` — used by the analyzer so the
/// estimated savings only count images that can actually shrink.
/// `cmyk_declared` marks a CMYK color space, which stays untouched under
/// the default settings (the conversion is opt-in).
#[allow(clippy::too_many_arguments)]
pub(crate) fn image_is_actionable(
    is_jpeg: bool,
    codec_supported: bool,
    bytes: u64,
    longest_edge: Option<u32>,
    pixels: Option<u64>,
    target_edge: u32,
    policy: SkipPolicy,
    cmyk_declared: bool,
) -> bool {
    if !codec_supported || cmyk_declared {
        return false;
    }
    if let Some(pixels) = pixels {
        if pixels <= TRIVIAL_PIXEL_COUNT {
            return false;
        }
    }
    if is_jpeg && bytes <= TINY_JPEG_STREAM_BYTES as u64 {
        return false;
    }
    if let Some(edge) = longest_edge {
        if edge <= target_edge {
            // Mirrors the two "already compact for the target" fast skips.
            if bytes <= policy.small_stream_bytes as u64 {
                return false;
            }
            if let Some(pixels) = pixels {
                if is_jpeg && pixels <= SMALL_JPEG_PIXEL_COUNT {
                    return false;
                }
            }
        }
    }
    true
}

/// Decide whether to recompress a single image stream. Returns quickly for
/// images that cannot benefit from recompression (fast-path skips). Decode and
/// encode failures are reported as skips with a reason; only cancellation
/// propagates as an error. Images with an 8-bit grayscale `/SMask` keep their
/// transparency: the color plane is re-encoded as JPEG and the alpha plane as
/// a flate-compressed grayscale soft mask.
///
/// The input streams are borrowed: on `Skipped` the caller puts its originals
/// back untouched, so no bytes are copied for the skip paths. In search mode
/// (`search_cache`) the decoded color plane and alpha products are cached
/// across target-size probe rounds — JPEG decode results do not depend on the
/// quality knob, so re-encoding a cached bitmap skips the expensive half of
/// the pipeline.
#[allow(clippy::too_many_arguments)]
pub(super) fn optimize_image_stream(
    stream: &Stream,
    smask: Option<&Stream>,
    settings: &CompressionSettings,
    cancel_flag: &Arc<AtomicBool>,
    task_id: &str,
    skip_policy: SkipPolicy,
    mut search_cache: Option<&mut ImageSearchCache>,
    color_space: Option<&ImageColorSpaceInfo>,
    jbig2_globals: Option<&[u8]>,
) -> Result<ImageOptimization, AppError> {
    ensure_not_cancelled(cancel_flag, task_id)?;
    // --- Skip: stencil image masks and color-key masks stay untouched ---
    if stream.dict.get(b"ImageMask").is_ok() {
        return Ok(ImageOptimization::Skipped {
            reason: "image masks (stencils) are not rewritten".into(),
        });
    }
    if stream.dict.get(b"Mask").is_ok() {
        return Ok(ImageOptimization::Skipped {
            reason: "color-key masks are not rewritten".into(),
        });
    }
    let has_smask = stream.dict.get(b"SMask").is_ok();
    if has_smask && smask.is_none() {
        // Includes the rare case of several images sharing one /SMask object:
        // the first task moves it out, later sharers find nothing and stay
        // untouched rather than risking a wrong rewrite.
        return Ok(ImageOptimization::Skipped {
            reason: "soft mask could not be resolved for a safe rewrite".into(),
        });
    }

    let filter_info = stream_filter_info(stream);

    // --- Skip: unsupported filters (JBIG2, non-decodable CCITT/JPX, Crypt) ---
    if filter_info.has_unsupported_filter {
        return Ok(ImageOptimization::Skipped {
            reason: "unsupported image filter for safe recompression".into(),
        });
    }

    // --- Fast skip: tiny JPEG streams ---
    if filter_info.has_jpeg && stream.content.len() <= TINY_JPEG_STREAM_BYTES {
        return Ok(ImageOptimization::Skipped {
            reason: "JPEG stream too small to benefit from recompression".into(),
        });
    }

    // --- Fast skip: trivially small images by pixel count ---
    if let Some(pixels) = pixel_count(stream) {
        if pixels <= TRIVIAL_PIXEL_COUNT {
            return Ok(ImageOptimization::Skipped {
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
            if longest <= max_edge && stream.content.len() <= skip_policy.small_stream_bytes {
                return Ok(ImageOptimization::Skipped {
                    reason: "JPEG already within target dimensions and stream size".into(),
                });
            }

            // Near target edge AND very small stream → skip for speed.
            let tolerance_edge = (max_edge as f32 * RESIZE_EDGE_TOLERANCE) as u32;
            if longest <= tolerance_edge
                && stream.content.len() <= skip_policy.small_stream_bytes / 2
            {
                return Ok(ImageOptimization::Skipped {
                    reason: "JPEG near target dimensions with small stream".into(),
                });
            }
        }
    }

    // --- Dictionary-based dimension check (catches non-JPEG too) ---
    if let Some(edge) = longest_edge(stream) {
        if filter_info.has_jpeg
            && edge <= max_edge
            && stream.content.len() <= skip_policy.small_stream_bytes
        {
            return Ok(ImageOptimization::Skipped {
                reason: "already below target size and unlikely to shrink".into(),
            });
        }
    }

    // --- Additional heuristic skip checks ---
    if let Some(reason) = skip_recompression_reason(stream, settings, filter_info, skip_policy) {
        return Ok(ImageOptimization::Skipped { reason });
    }

    // --- CMYK fidelity gate (opt-in conversion) ---
    // The naive ink-subtraction conversion deviates measurably from how
    // color-managed renderers interpret CMYK, so CMYK images stay untouched
    // unless the user opted in (or requested a grayscale/G4 collapse, where
    // the deviation is far below the intent's own loss). Applies to every
    // path: raw and indexed planes here, CMYK JPEGs (the JPEG decoder
    // silently converts), and 4-component JPX codestreams at decode.
    if declares_cmyk(stream, color_space) && !settings.converts_cmyk() {
        return Ok(ImageOptimization::Skipped {
            reason: "CMYK image kept untouched (enable the CMYK conversion setting to re-encode)".into(),
        });
    }

    if let Some(reason) = raw_recompression_skip_reason(stream, filter_info, color_space) {
        return Ok(ImageOptimization::Skipped { reason });
    }

    let original_len = stream.content.len();

    // --- Decode the color plane (or take it back from the search cache) ---
    // Decode/encode failures are treated as skips, not fatal errors: one
    // malformed image stream in a hostile or damaged PDF must not abort the
    // whole file. Only cancellation propagates as `Err`.
    ensure_not_cancelled(cancel_flag, task_id)?;

    let cached_plane = search_cache
        .as_deref_mut()
        .and_then(|cache| cache.bitmap.take());
    let mut plane = match cached_plane {
        Some(cached) => cached,
        None => {
            if let Some(reason) = search_cache
                .as_ref()
                .and_then(|cache| cache.undecodable_reason.clone())
            {
                return Ok(ImageOptimization::Skipped { reason });
            }
            let decoded = if filter_info.has_jpeg {
                image::load_from_memory(&stream.content)
                    .map_err(|e| format!("failed to decode JPEG image stream: {e}"))
            } else if filter_info.has_ccitt {
                #[cfg(feature = "ccitt")]
                {
                    decode_ccitt_stream(stream)
                        .map_err(|e| format!("failed to decode CCITT image stream: {e}"))
                }
                #[cfg(not(feature = "ccitt"))]
                {
                    // Unreachable: non-decodable CCITT was skipped above.
                    Err("CCITT support is not compiled in".to_string())
                }
            } else if filter_info.has_jpx {
                #[cfg(feature = "jpx")]
                {
                    super::jpx::decode_jpx_stream(stream, settings.converts_cmyk())
                        .map_err(|e| format!("failed to decode JPX image stream: {e}"))
                }
                #[cfg(not(feature = "jpx"))]
                {
                    // Unreachable: non-decodable JPX was skipped above.
                    Err("JPX support is not compiled in".to_string())
                }
            } else if filter_info.has_jbig2 {
                super::jbig2::decode_jbig2_stream(stream, jbig2_globals)
                    .map_err(|e| format!("failed to decode JBIG2 image stream: {e}"))
            } else {
                decode_raw_image_stream(stream, color_space)
                    .map_err(|e| format!("failed to decode raw image stream: {e}"))
            };
            match decoded {
                Ok(image) => {
                    // Grayscale conversion happens before resizing: it is
                    // constant across probe rounds (so the cached plane stays
                    // gray) and resizing a single-channel plane is a third of
                    // the work of RGB. G4 output implies luma as well.
                    let wants_luma =
                        settings.grayscale || settings.bilevel_codec.uses_ccitt();
                    if wants_luma && image.color().has_color() {
                        DynamicImage::ImageLuma8(image.to_luma8())
                    } else {
                        image
                    }
                }
                Err(reason) => {
                    // Remember undecodable streams so later rounds skip retries.
                    if let Some(cache) = search_cache.as_deref_mut() {
                        cache.undecodable_reason = Some(reason.clone());
                    }
                    return Ok(ImageOptimization::Skipped { reason });
                }
            }
        }
    };

    // --- Resize if needed (two-pass for large images) ---
    ensure_not_cancelled(cancel_flag, task_id)?;
    plane = resize_if_needed_fast(plane, settings.max_image_size_px);

    let color_space_name = if plane.color().has_color() {
        "DeviceRGB"
    } else {
        "DeviceGray"
    };

    // --- Decode + resize the alpha plane to match the color plane ---
    let new_smask = if has_smask {
        match smask_stream_for_round(smask, &mut search_cache, (plane.width(), plane.height())) {
            Some(smask_stream) => Some(smask_stream),
            None => {
                return Ok(ImageOptimization::Skipped {
                    reason: "soft mask uses an unsupported shape for a safe rewrite".into(),
                })
            }
        }
    } else {
        None
    };

    // --- Encode as JPEG (or CCITT G4 for near-bilevel planes) ---
    // Pre-allocate based on conservative compression ratio estimate.
    ensure_not_cancelled(cancel_flag, task_id)?;
    let plane_dimensions = (plane.width(), plane.height());

    // Target-size search rounds only: redistribute the round's quality across
    // images by plane detail (see `search_round_quality`). Outside the search
    // the user's quality number is applied verbatim. The plane is a
    // deterministic function of (stream, edge, grayscale), so probe estimates
    // and materialized encodings stay byte-identical.
    let encode_quality = if search_cache.is_some() {
        search_round_quality(settings.image_quality, &plane)
    } else {
        settings.image_quality
    };

    #[cfg(feature = "ccitt")]
    let bilevel_g4 =
        settings.bilevel_codec.uses_ccitt() && plane_is_near_bilevel(&plane);
    #[cfg(not(feature = "ccitt"))]
    let bilevel_g4 = false;

    let encoded: Vec<u8> = if bilevel_g4 {
        #[cfg(feature = "ccitt")]
        {
            g4_bytes_for_round(&plane, &mut search_cache)
        }
        #[cfg(not(feature = "ccitt"))]
        {
            unreachable!("bilevel_g4 is constant false without the ccitt feature")
        }
    } else {
        let estimated_output_size = (original_len as f32 * 0.65) as usize;
        match encode_dynamic_image_as_jpeg(&plane, encode_quality, estimated_output_size) {
            Ok(encoded) => encoded,
            Err(e) => {
                return Ok(ImageOptimization::Skipped {
                    reason: format!("failed to re-encode image as JPEG: {e}"),
                })
            }
        }
    };

    // Hand the (possibly pre-shrunk) plane to the cache for later rounds.
    // This is the final use of `search_cache`, so it is consumed here.
    let plane_channels: u8 = if plane.color().has_color() { 3 } else { 1 };
    if let Some(cache) = search_cache {
        cache.store_bitmap(plane);
    }

    // --- Skip if the new encoding is not smaller AND we didn't resize ---
    if encoded.len() >= original_len && longest_edge(stream).is_some_and(|edge| edge <= max_edge) {
        return Ok(ImageOptimization::Skipped {
            reason: "existing image stream is already compact for the requested target".into(),
        });
    }

    // --- Build the rebuilt stream from the original dictionary ---
    // ICC-based originals keep their profile reference when the encoded
    // plane still matches the declared channel count, instead of silently
    // degrading to a generic Device name.
    let restored_color_space = color_space
        .and_then(|info| info.rebuild_color_space.as_ref())
        .filter(|(_, channels)| *channels == plane_channels);

    let mut rebuilt = Stream::new(stream.dict.clone(), encoded);
    if bilevel_g4 {
        rebuilt
            .dict
            .set("Filter", Object::Name(b"CCITTFaxDecode".to_vec()));
        rebuilt.dict.set(
            "DecodeParms",
            Object::Dictionary(dictionary! {
                "K" => -1,
                "Columns" => i64::from(plane_dimensions.0),
                "Rows" => i64::from(plane_dimensions.1),
                "BlackIs1" => true,
            }),
        );
        rebuilt.dict.set("BitsPerComponent", Object::Integer(1));
        match restored_color_space {
            Some((object, _)) => rebuilt.dict.set("ColorSpace", object.clone()),
            None => rebuilt
                .dict
                .set("ColorSpace", Object::Name(b"DeviceGray".to_vec())),
        }
    } else {
        rebuilt
            .dict
            .set("Filter", Object::Name(b"DCTDecode".to_vec()));
        rebuilt.dict.remove(b"DecodeParms");
        rebuilt.dict.set("BitsPerComponent", Object::Integer(8));
        match restored_color_space {
            Some((object, _)) => rebuilt.dict.set("ColorSpace", object.clone()),
            None => rebuilt.dict.set(
                "ColorSpace",
                Object::Name(color_space_name.as_bytes().to_vec()),
            ),
        }
    }
    rebuilt
        .dict
        .set("Width", Object::Integer(i64::from(plane_dimensions.0)));
    rebuilt
        .dict
        .set("Height", Object::Integer(i64::from(plane_dimensions.1)));

    Ok(ImageOptimization::Recompressed {
        stream: rebuilt,
        smask: new_smask,
    })
}

/// Build (or reuse) the flate-compressed grayscale soft-mask stream matching
/// the color plane's dimensions. Returns `None` when the alpha plane has an
/// unsupported shape — the caller must then skip the image.
fn smask_stream_for_round(
    smask: Option<&Stream>,
    cache: &mut Option<&mut ImageSearchCache>,
    dims: (u32, u32),
) -> Option<Stream> {
    // Quality-only probe rounds re-encode the color plane but reproduce the
    // exact same alpha bytes — memoized by target dimensions.
    if let Some((cached_dims, cached)) = cache
        .as_ref()
        .and_then(|cache| cache.smask_product.as_ref())
    {
        if *cached_dims == dims {
            return Some(cached.clone());
        }
    }

    let cached_gray = cache
        .as_deref_mut()
        .and_then(|cache| cache.smask_gray.take());
    let decoded = match cached_gray {
        Some(cached) => cached,
        None => {
            let decoded = smask.and_then(decode_smask_gray);
            if let Some(cache) = cache.as_deref_mut() {
                cache.smask_gray = Some(decoded.clone());
            }
            decoded
        }
    };
    let resized = resize_smask_to(decoded?, dims.0, dims.1);
    let stream = build_smask_stream(&resized);
    if let Some(cache) = cache.as_deref_mut() {
        cache.smask_product = Some((dims, stream.clone()));
    }
    Some(stream)
}

/// Wrap a resized alpha plane as a flate-compressed DeviceGray image stream.
fn build_smask_stream(alpha: &image::GrayImage) -> Stream {
    let mut soft_mask = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(alpha.width()),
            "Height" => i64::from(alpha.height()),
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
        },
        alpha.as_raw().clone(),
    );
    let _ = soft_mask.compress();
    soft_mask
}

// ---------------------------------------------------------------------------
// Soft-mask (transparency) helpers
// ---------------------------------------------------------------------------

/// Decode an `/SMask` stream into an 8-bit grayscale image. Returns `None`
/// for any shape we cannot rewrite safely (non-gray, non-8bit, mismatched
/// byte counts, undecodable filter, an unsupported `/Decode` mapping).
/// A `/Decode` of `[1 0]` inverts the samples — the values are normalized
/// here so the rebuilt mask (which never carries `/Decode`) preserves the
/// original opacity.
fn decode_smask_gray(smask: &Stream) -> Option<image::GrayImage> {
    let width = optional_integer(smask, b"Width")? as u32;
    let height = optional_integer(smask, b"Height")? as u32;
    if width == 0 || height == 0 {
        return None;
    }

    if optional_integer(smask, b"BitsPerComponent").unwrap_or(8) != 8 {
        return None;
    }

    if let Ok(Object::Name(color_space)) = smask.dict.get(b"ColorSpace") {
        if color_space.as_slice() != b"DeviceGray" {
            return None;
        }
    }

    let inverted = match smask.dict.get(b"Decode") {
        // Identity (or absent): samples map straight to opacity.
        Ok(Object::Array(items)) if items.len() == 2 => {
            let as_number = |item: &Object| match item {
                Object::Integer(value) => Some(*value as f64),
                Object::Real(value) => Some(f64::from(*value)),
                _ => None,
            };
            match (as_number(&items[0]), as_number(&items[1])) {
                (Some(0.0), Some(1.0)) => false,
                (Some(1.0), Some(0.0)) => true,
                // Partial ranges would rescale the ramp — not a shape this
                // rewrite can preserve honestly.
                _ => return None,
            }
        }
        Err(_) => false,
        Ok(_) => return None,
    };

    // A mask without /Filter is spec-legal (raw bytes); get_plain_content
    // handles both raw and flate-encoded shapes.
    let mut data = smask.get_plain_content().ok()?;
    if inverted {
        data.iter_mut().for_each(|sample| *sample = 255 - *sample);
    }
    image::GrayImage::from_raw(width, height, data)
}

/// Resize the alpha plane to exactly match the color plane dimensions.
fn resize_smask_to(alpha: image::GrayImage, width: u32, height: u32) -> image::GrayImage {
    let dynamic = DynamicImage::ImageLuma8(alpha);
    let resized = if dynamic.width() == width && dynamic.height() == height {
        dynamic
    } else {
        dynamic.resize_exact(width, height, FilterType::CatmullRom)
    };

    match resized {
        DynamicImage::ImageLuma8(gray) => gray,
        other => other.to_luma8(),
    }
}

// ---------------------------------------------------------------------------
// Skip heuristics
// ---------------------------------------------------------------------------

fn skip_recompression_reason(
    stream: &Stream,
    settings: &CompressionSettings,
    filter_info: StreamFilterInfo,
    skip_policy: SkipPolicy,
) -> Option<String> {
    let edge = longest_edge(stream)?;
    if edge > u32::from(settings.max_image_size_px) {
        return None;
    }

    if stream.content.len() <= skip_policy.small_stream_bytes {
        return Some("already below target and unlikely to shrink meaningfully".into());
    }

    let pixels = pixel_count(stream)?;
    if filter_info.has_jpeg && pixels <= SMALL_JPEG_PIXEL_COUNT {
        return Some("already near the requested target dimensions".into());
    }

    None
}

/// Does this stream declare a CMYK color space — resolved (ICC N=4,
/// CMYK-based Indexed) or by a plain `/DeviceCMYK` name (covers CMYK JPEGs
/// and raw planes alike)? Shared by the compressor gate and the analyzer's
/// actionable mirror.
pub(crate) fn declares_cmyk(
    stream: &Stream,
    color_space: Option<&ImageColorSpaceInfo>,
) -> bool {
    if let Some(info) = color_space {
        return match &info.decode {
            DecodeColorSpace::Cmyk => true,
            DecodeColorSpace::Indexed { base_channels, .. } => *base_channels == 4,
            _ => false,
        };
    }
    matches!(
        stream.dict.get(b"ColorSpace"),
        Ok(Object::Name(name)) if name.as_slice() == b"DeviceCMYK"
    )
}

fn raw_recompression_skip_reason(
    stream: &Stream,
    filter_info: StreamFilterInfo,
    color_space: Option<&ImageColorSpaceInfo>,
) -> Option<String> {
    if filter_info.has_jpeg {
        return None;
    }
    // CCITT, JPX, and JBIG2 streams go through their own decoders; the
    // raw-plane checks below (bit depth, name color space) do not apply.
    if filter_info.has_ccitt || filter_info.has_jpx || filter_info.has_jbig2 {
        return None;
    }

    let bits_per_component = optional_integer(stream, b"BitsPerComponent").unwrap_or(8);

    // Resolved shapes (ICC-based, indexed, aliases) carry their own rules:
    // indexed pixels are 4/8-bit palette indices, ICC planes stay 8-bit.
    if let Some(info) = color_space {
        if info.decode == DecodeColorSpace::Cmyk && stream.dict.get(b"Decode").is_ok() {
            return Some(
                "CMYK image carries a /Decode mapping that the RGB conversion would misread"
                    .into(),
            );
        }
        let supported_depth = match info.decode {
            DecodeColorSpace::Indexed { .. } => bits_per_component == 4 || bits_per_component == 8,
            DecodeColorSpace::Gray | DecodeColorSpace::Rgb | DecodeColorSpace::Cmyk => {
                bits_per_component == 8
            }
        };
        return (!supported_depth).then(|| {
            format!("resolved color space needs unsupported bit depth: {bits_per_component}")
        });
    }

    if bits_per_component != 8 {
        return Some(format!(
            "raw image uses unsupported bit depth for safe recompression: {bits_per_component}"
        ));
    }

    // Only name-valued device color spaces are safely decodable.
    // An absent ColorSpace defaults to DeviceRGB per the PDF spec; anything
    // else (arrays such as [/ICCBased …] or [/Indexed …]) is skipped because
    // the raw bytes would be misinterpreted.
    match stream.dict.get(b"ColorSpace") {
        Ok(Object::Name(name)) => match name.as_slice() {
            b"DeviceGray" | b"DeviceRGB" => None,
            // A /Decode mapping reinterprets the samples before rendering;
            // feeding remapped CMYK through the ink-subtraction conversion
            // would shift colors silently.
            b"DeviceCMYK" if stream.dict.get(b"Decode").is_err() => None,
            other => Some(format!(
                "raw image uses unsupported color space for safe recompression: {}",
                String::from_utf8_lossy(other)
            )),
        },
        Ok(_) => Some(
            "raw image uses a non-name color space (ICC, indexed, …) that is not safely rewritable"
                .to_string(),
        ),
        Err(_) => None,
    }
}

// ---------------------------------------------------------------------------
// Resize logic
// ---------------------------------------------------------------------------

/// Fast, two-pass resize strategy optimized for throughput:
///
/// 1. If the image is within the edge tolerance, return as-is (no work).
/// 2. SIMD path: for the layouts `fast_image_resize` supports (exactly the
///    Luma8/Rgb8-family planes this pipeline produces), a single
///    AVX2/SSE4-accelerated CatmullRom pass — typically 3–8× faster than the
///    scalar `image` crate on large images, which also makes the old
///    two-pass Nearest trick unnecessary.
/// 3. Scalar fallback: two-pass (Nearest bulk downsample + CatmullRom refine)
///    above the pixel threshold, single-pass CatmullRom otherwise.
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

    // SIMD path with the fast_image_resize crate; falls through on any
    // unsupported layout or internal error.
    if let Some(resized) = try_simd_resize(&image, target_w, target_h) {
        return resized;
    }

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

/// Single-pass SIMD resize for the pixel layouts this pipeline produces.
/// Returns `None` when the layout is unsupported (the caller falls back to
/// the scalar `image` crate path). `fast_image_resize` picks the best CPU
/// instruction set at runtime and degrades gracefully.
fn try_simd_resize(image: &DynamicImage, target_w: u32, target_h: u32) -> Option<DynamicImage> {
    use fast_image_resize as simd;

    let mut destination = match image {
        DynamicImage::ImageLuma8(_) => DynamicImage::new_luma8(target_w, target_h),
        DynamicImage::ImageLumaA8(_) => DynamicImage::new_luma_a8(target_w, target_h),
        DynamicImage::ImageRgb8(_) => DynamicImage::new_rgb8(target_w, target_h),
        DynamicImage::ImageRgba8(_) => DynamicImage::new_rgba8(target_w, target_h),
        _ => return None,
    };

    let options = simd::ResizeOptions::new()
        .resize_alg(simd::ResizeAlg::Convolution(simd::FilterType::CatmullRom));
    let mut resizer = simd::Resizer::new();
    resizer
        .resize(image, &mut destination, &options)
        .ok()
        .map(|()| destination)
}

// ---------------------------------------------------------------------------
// Image decode / encode helpers
// ---------------------------------------------------------------------------

/// Decode a raw (non-JPEG) image stream using PDF dictionary metadata.
/// `color_space` carries document-level context resolved up front (ICC
/// channel counts, indexed palettes); without it only plain device-name
/// color spaces are decodable.
pub(super) fn decode_raw_image_stream(
    stream: &Stream,
    color_space: Option<&ImageColorSpaceInfo>,
) -> Result<DynamicImage, AppError> {
    let width = required_integer(stream, b"Width")? as u32;
    let height = required_integer(stream, b"Height")? as u32;

    let decoded = stream
        .decompressed_content()
        .map_err(|e| AppError::PdfBuild(format!("Failed to decompress raw image stream: {e}")))?;

    if let Some(info) = color_space {
        return match &info.decode {
            DecodeColorSpace::Gray => image::GrayImage::from_raw(width, height, decoded)
                .map(DynamicImage::ImageLuma8)
                .ok_or_else(|| {
                    AppError::PdfBuild(
                        "Grayscale image bytes did not match the declared dimensions.".into(),
                    )
                }),
            DecodeColorSpace::Rgb => image::RgbImage::from_raw(width, height, decoded)
                .map(DynamicImage::ImageRgb8)
                .ok_or_else(|| {
                    AppError::PdfBuild("RGB image bytes did not match the declared dimensions.".into())
                }),
            DecodeColorSpace::Cmyk => cmyk_bytes_to_rgb_image(width, height, decoded),
            DecodeColorSpace::Indexed {
                base_channels,
                palette,
            } => {
                let channels = usize::from(*base_channels);
                if !matches!(channels, 1 | 3 | 4) {
                    return Err(AppError::PdfBuild(
                        "Indexed image uses an unsupported base color space.".into(),
                    ));
                }
                let bits = optional_integer(stream, b"BitsPerComponent").unwrap_or(8);
                let indices =
                    super::colorspace::unpack_indices(&decoded, bits, width).ok_or_else(|| {
                    AppError::PdfBuild(
                        "Indexed image uses an unsupported index bit depth.".into(),
                    )
                })?;
                let mut pixels = Vec::with_capacity(indices.len() * channels);
                for index in indices {
                    let offset = index as usize * channels;
                    let entry = palette
                        .get(offset..offset + channels)
                        .ok_or_else(|| AppError::PdfBuild("Indexed palette lookup out of range.".into()))?;
                    pixels.extend_from_slice(entry);
                }
                if channels == 1 {
                    image::GrayImage::from_raw(width, height, pixels)
                        .map(DynamicImage::ImageLuma8)
                } else if channels == 3 {
                    image::RgbImage::from_raw(width, height, pixels)
                        .map(DynamicImage::ImageRgb8)
                } else {
                    cmyk_bytes_to_rgb_image(width, height, pixels).ok()
                }
                .ok_or_else(|| {
                    AppError::PdfBuild(
                        "Indexed image bytes did not match the declared dimensions.".into(),
                    )
                })
            }
        };
    }

    let color_space =
        optional_name(stream, b"ColorSpace").unwrap_or_else(|| "DeviceRGB".to_string());

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
        "DeviceCMYK" => cmyk_bytes_to_rgb_image(width, height, decoded),
        other => Err(AppError::PdfBuild(format!(
            "Unsupported color space for recompression: {other}"
        ))),
    }
}

/// Convert interleaved 8-bit CMYK bytes into an RGB plane via the shared
/// ink-subtraction conversion (see `colorspace::cmyk_to_rgb`). A length that
/// is not an exact `width × height × 4` fails the `from_raw` shape check,
/// matching the gray/RGB paths' dimension strictness.
fn cmyk_bytes_to_rgb_image(
    width: u32,
    height: u32,
    decoded: Vec<u8>,
) -> Result<DynamicImage, AppError> {
    let mut pixels = Vec::with_capacity(decoded.len() / 4 * 3);
    for sample in decoded.chunks_exact(4) {
        let [red, green, blue] =
            super::colorspace::cmyk_to_rgb(sample[0], sample[1], sample[2], sample[3]);
        pixels.extend_from_slice(&[red, green, blue]);
    }
    image::RgbImage::from_raw(width, height, pixels)
        .map(DynamicImage::ImageRgb8)
        .ok_or_else(|| {
            AppError::PdfBuild(
                "CMYK image bytes did not match the declared dimensions.".into(),
            )
        })
}

/// Encode a `DynamicImage` as JPEG into a `Vec<u8>`.
///
/// Uses the jpeg-encoder crate (SIMD build): measured ~3× faster than
/// image's built-in encoder on photographic content with slightly smaller
/// output at the same quality number (see `benches/encoder.rs`). Progressive
/// scans plus optimized Huffman tables typically shave another 5–15% at the
/// same visual quality for a small CPU cost.
fn encode_dynamic_image_as_jpeg(
    image: &DynamicImage,
    quality: u8,
    expected_capacity: usize,
) -> Result<Vec<u8>, AppError> {
    use std::borrow::Cow;

    let (color_type, pixels): (jpeg_encoder::ColorType, Cow<'_, [u8]>) = match image {
        DynamicImage::ImageLuma8(gray) => {
            (jpeg_encoder::ColorType::Luma, Cow::Borrowed(gray.as_raw()))
        }
        DynamicImage::ImageRgb8(rgb) => (jpeg_encoder::ColorType::Rgb, Cow::Borrowed(rgb.as_raw())),
        other => (
            jpeg_encoder::ColorType::Rgb,
            Cow::Owned(other.to_rgb8().into_raw()),
        ),
    };

    let (width, height) = (image.width(), image.height());
    if width > u16::MAX as u32 || height > u16::MAX as u32 {
        return Err(AppError::PdfBuild(
            "Image dimensions exceed the JPEG format limit.".into(),
        ));
    }

    // At least 32 KB to avoid re-allocation on small images.
    let capacity = expected_capacity.max(32 * 1024);
    let mut output = Vec::with_capacity(capacity);
    let mut encoder = jpeg_encoder::Encoder::new(&mut output, quality);
    encoder.set_progressive(true);
    encoder.set_optimized_huffman_tables(true);
    encoder
        .encode(pixels.as_ref(), width as u16, height as u16, color_type)
        .map_err(|e| AppError::PdfBuild(format!("Failed to encode JPEG: {e}")))?;
    Ok(output)
}

// ---------------------------------------------------------------------------
// CCITT Group 4 (bi-level) codec helpers
// ---------------------------------------------------------------------------

/// Decodable CCITT input shape, shared by the filter gate and the decoder
/// dispatch. Group 3 support covers the one-dimensional (K = 0) coding used
/// by legacy fax scans: rows are plain MH run-lengths, optionally EOL-
/// delimited (TIFF-style) or byte-aligned.
#[cfg(feature = "ccitt")]
enum CcittInputShape {
    /// Group 4 (K < 0), no byte alignment.
    Group4 {
        width: u32,
        height: u32,
        black_is_1: bool,
    },
    /// Group 3 one-dimensional (K = 0). `end_of_line` rows are decoded by
    /// fax's `decode_g3` (fill bits + EOL + RTC aware); plain rows use the
    /// local no-EOL reader, which also honors `EncodedByteAlign`. The rare
    /// EOL-plus-alignment combination is not supported (the align padding
    /// between an EOL and the next row's codes is ambiguous).
    Group3OneDim {
        width: u32,
        height: u32,
        black_is_1: bool,
        end_of_line: bool,
        byte_align: bool,
    },
}

/// Validate that a CCITT stream uses a shape this engine can decode. Returns
/// the shape on success. Group 3 two-dimensional coding (K > 0), indirect
/// `/DecodeParms`, flate-mixed chains, and oversized dimensions stay
/// unsupported.
#[cfg(feature = "ccitt")]
fn ccitt_input_shape(stream: &Stream) -> Option<CcittInputShape> {
    let width = optional_integer(stream, b"Width")?;
    let height = optional_integer(stream, b"Height")?;
    // Never let hostile dimensions reach the decoded-plane allocation: a
    // negative i64 would wrap to a huge value at the `as u32` cast. The
    // ceiling matches the JPEG format limit the encoder already enforces —
    // a bigger plane could never be re-encoded anyway.
    if !(1..=65_535).contains(&width) || !(1..=65_535).contains(&height) {
        return None;
    }
    let (width, height) = (width as u32, height as u32);

    // An absent /DecodeParms defaults to K = 0 — Group 3 one-dimensional
    // without EOL markers, which the local reader handles. Indirect parameter
    // references are rare and are not resolved in this stream-local context.
    let parms = match stream.dict.get(b"DecodeParms") {
        Ok(Object::Dictionary(dict)) => Some(dict),
        Err(_) => None,
        Ok(_) => return None,
    };

    // A /Decode array re-interprets the decoded samples before rendering;
    // the row expanders assume only the BlackIs1 mapping, so such streams
    // stay untouched rather than silently flipping polarity.
    if stream.dict.get(b"Decode").is_ok() {
        return None;
    }

    let flag = |dict: Option<&Dictionary>, key: &[u8]| {
        dict.and_then(|dict| match dict.get(key) {
            Ok(Object::Boolean(value)) => Some(*value),
            _ => None,
        })
        .unwrap_or(false)
    };

    let black_is_1 = flag(parms, b"BlackIs1");
    let byte_align = flag(parms, b"EncodedByteAlign");
    let k = parms
        .and_then(|dict| optional_dict_integer(dict, b"K"))
        .unwrap_or(0);

    match k {
        0 => {
            let end_of_line = flag(parms, b"EndOfLine");
            if end_of_line && byte_align {
                return None;
            }
            Some(CcittInputShape::Group3OneDim {
                width,
                height,
                black_is_1,
                end_of_line,
                byte_align,
            })
        }
        k if k < 0 => {
            if byte_align {
                return None;
            }
            Some(CcittInputShape::Group4 {
                width,
                height,
                black_is_1,
            })
        }
        _ => None,
    }
}

/// Decode a CCITT image stream (Group 4 or one-dimensional Group 3) into an
/// 8-bit grayscale plane (black = 0, white = 255).
#[cfg(feature = "ccitt")]
fn decode_ccitt_stream(stream: &Stream) -> Result<DynamicImage, AppError> {
    match ccitt_input_shape(stream).ok_or_else(|| {
        AppError::PdfBuild("CCITT stream does not use a supported decode shape".into())
    })? {
        CcittInputShape::Group4 {
            width,
            height,
            black_is_1,
        } => decode_ccitt_g4_rows(stream, width, height, black_is_1),
        CcittInputShape::Group3OneDim {
            width,
            height,
            black_is_1,
            end_of_line: true,
            byte_align: false,
        } => decode_ccitt_g3_eol(stream, width, height, black_is_1),
        CcittInputShape::Group3OneDim {
            width,
            height,
            black_is_1,
            end_of_line: false,
            byte_align,
        } => decode_ccitt_g3_plain(stream, width, height, black_is_1, byte_align),
        // The EOL-plus-alignment combination is filtered out by
        // `ccitt_input_shape`; this arm only satisfies exhaustiveness.
        CcittInputShape::Group3OneDim {
            end_of_line: true,
            byte_align: true,
            ..
        } => Err(AppError::PdfBuild(
            "CCITT Group 3 EOL with byte alignment is not supported".into(),
        )),
    }
}

/// Group 4 path: the fax crate emits per-line color transitions; `pels`
/// expands them back into full rows.
#[cfg(feature = "ccitt")]
fn decode_ccitt_g4_rows(
    stream: &Stream,
    width: u32,
    height: u32,
    black_is_1: bool,
) -> Result<DynamicImage, AppError> {
    let mut pixels = vec![0u8; width as usize * height as usize];
    let mut written_rows = 0usize;

    let decoded = fax::decoder::decode_g4(
        stream.content.iter().copied(),
        width,
        Some(height),
        |transitions| {
            write_transitions_row(
                &mut pixels[written_rows * width as usize..][..width as usize],
                transitions,
                black_is_1,
            );
            written_rows += 1;
        },
    );

    if decoded.is_none() {
        return Err(AppError::PdfBuild(
            "failed to decode CCITT Group 4 image stream".into(),
        ));
    }

    image::GrayImage::from_vec(width, height, pixels)
        .map(DynamicImage::ImageLuma8)
        .ok_or_else(|| AppError::PdfBuild("CCITT decode produced mismatched buffer".into()))
}

/// Group 3 with EOL markers: fax's `decode_g3` handles fill bits, EOL
/// delimiters, and the RTC (six EOLs) terminator. Rows beyond the declared
/// height are dropped; trailing garbage after the RTC is tolerated.
#[cfg(feature = "ccitt")]
fn decode_ccitt_g3_eol(
    stream: &Stream,
    width: u32,
    height: u32,
    black_is_1: bool,
) -> Result<DynamicImage, AppError> {
    let mut pixels = vec![0u8; width as usize * height as usize];
    let mut written_rows = 0usize;

    let decoded = fax::decoder::decode_g3(
        stream.content.iter().copied(),
        |transitions| {
            if written_rows < height as usize {
                // `decode_g3` does NOT swap the fax Color labels the way the
                // G4 path does (see `write_transitions_row`): a fax-White run
                // is a true white run here, so the flag is flipped to undo
                // the G4-oriented polarity inside the row expander.
                write_transitions_row(
                    &mut pixels[written_rows * width as usize..][..width as usize],
                    transitions,
                    !black_is_1,
                );
                written_rows += 1;
            }
        },
    );

    if written_rows < height as usize && decoded.is_none() {
        return Err(AppError::PdfBuild(
            "failed to decode CCITT Group 3 image stream".into(),
        ));
    }

    image::GrayImage::from_vec(width, height, pixels)
        .map(DynamicImage::ImageLuma8)
        .ok_or_else(|| AppError::PdfBuild("CCITT decode produced mismatched buffer".into()))
}

/// Expand one row of color-transition positions into 8-bit pixels.
///
/// Polarity model (verified against poppler, see the `g4_*` tests): the fax
/// crate's `Color::White` runs are **1-bits** — the `BlackIs1 = false`
/// convention (1 = white). With `BlackIs1 = true` a renderer maps 1-bits to
/// black, so a fax-`White` run renders black there. Runs past the row width
/// are truncated, short rows pad with the row's final run color (both
/// per-spec behaviors).
#[cfg(feature = "ccitt")]
fn write_transitions_row(row: &mut [u8], transitions: &[u32], black_is_1: bool) {
    let white_value: u8 = if black_is_1 { 0 } else { 255 };
    let black_value: u8 = 255 - white_value;
    let mut cursor = 0usize;
    let mut color = fax::Color::White;
    for &position in transitions {
        let end = (position as usize).min(row.len());
        let value = if color == fax::Color::Black {
            black_value
        } else {
            white_value
        };
        if end > cursor {
            row[cursor..end].fill(value);
            cursor = end;
        }
        color = !color;
    }
    // The tail extends the color the transition walk has flipped into — for
    // a whole-row run (a lone transition at 0) that is the run after the
    // zero-length one, and for short rows it continues the final run.
    let tail_value = if color == fax::Color::Black {
        black_value
    } else {
        white_value
    };
    row[cursor..].fill(tail_value);
}

/// Group 3 one-dimensional rows without EOL markers — PDF's default shape
/// (EndOfLine false). Reads alternating white/black MH run lengths straight
/// off the bitstream; `EncodedByteAlign` pads each completed row to the byte
/// boundary.
#[cfg(feature = "ccitt")]
fn decode_ccitt_g3_plain(
    stream: &Stream,
    width: u32,
    height: u32,
    black_is_1: bool,
    byte_align: bool,
) -> Result<DynamicImage, AppError> {
    use fax::BitReader as _;

    // This reader consumes the code tables directly (no fax Color labels),
    // so a `White`-table run is a true T.4 white-pixel run: with
    // `BlackIs1 = true` (0 = white) it renders 255. The transitions-based
    // paths above invert this because the fax crate binds its Color labels
    // to the opposite tables (see `write_transitions_row`).
    let white_value: u8 = if black_is_1 { 255 } else { 0 };
    let black_value: u8 = 255 - white_value;
    // fax's code tables need a few bits of lookahead past the final code — a
    // stream ending exactly at the last row would otherwise fail to decode
    // its own tail. Trailing zeros are spec-legal fill and never form a valid
    // code prefix, so genuinely truncated streams still fail cleanly.
    let mut content = stream.content.clone();
    content.extend_from_slice(&[0u8; 4]);
    let mut reader = fax::slice_reader(&content);
    let mut pixels = vec![0u8; width as usize * height as usize];

    for row in pixels.chunks_mut(width as usize) {
        let mut color = fax::Color::White;
        let mut written = 0usize;
        while written < row.len() {
            let run = read_markup_run(&mut reader, color).ok_or_else(|| {
                AppError::PdfBuild("failed to decode CCITT Group 3 run lengths".into())
            })?;
            let value = if color == fax::Color::Black {
                black_value
            } else {
                white_value
            };
            let end = (written + run as usize).min(row.len());
            row[written..end].fill(value);
            written = end;
            color = !color;
        }

        if byte_align {
            let padding = reader.bits_to_byte_boundary();
            if reader.consume(padding).is_err() {
                return Err(AppError::PdfBuild(
                    "CCITT Group 3 stream ended mid-row".into(),
                ));
            }
        }
    }

    Ok(DynamicImage::ImageLuma8(
        image::GrayImage::from_vec(width, height, pixels)
            .ok_or_else(|| AppError::PdfBuild("CCITT decode produced mismatched buffer".into()))?,
    ))
}

/// One MH run length: accumulate makeup codes (multiples of 64) until a
/// terminating code (< 64) lands. `None` on stream exhaustion or an invalid
/// code — both leave the image undecodable.
#[cfg(feature = "ccitt")]
fn read_markup_run(
    reader: &mut impl fax::BitReader,
    color: fax::Color,
) -> Option<u32> {
    let mut sum: u32 = 0;
    loop {
        let markup = match color {
            fax::Color::Black => fax::maps::black::decode(reader)?,
            fax::Color::White => fax::maps::white::decode(reader)?,
        };
        sum = sum.checked_add(u32::from(markup))?;
        if markup < 64 {
            return Some(sum);
        }
    }
}

/// Encode an 8-bit grayscale plane as CCITT Group 4 (ITU T.6). Pixels at or
/// above the 128 luma threshold become white, everything else black. The
/// fax crate's `Color::White` runs are **1-bits** (the `BlackIs1 = false`
/// convention, 1 = white), but the rebuilt stream declares `BlackIs1: true`
/// where 1-bits must be black — so the color fed to the encoder is the
/// *inverted* plane polarity, making white pixels ride 0-bit runs. Verified
/// against poppler in `g4_output_renders_white_background_as_white`.
#[cfg(feature = "ccitt")]
fn encode_gray_as_ccitt_g4(plane: &image::GrayImage) -> Vec<u8> {
    let (width, height) = plane.dimensions();
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::with_capacity(
        (width as usize * height as usize) / 16,
    ));

    for row in plane.as_raw().chunks(width as usize) {
        // VecWriter's error type is `Infallible` — there is nothing to handle.
        let _ = encoder.encode_line(
            row.iter()
                .map(|&luma| if luma >= 128 { fax::Color::Black } else { fax::Color::White }),
            width,
        );
    }

    // `finish` appends the end-of-facsimile-block marker.
    let writer = match encoder.finish() {
        Ok(writer) => writer,
        Err(infallible) => match infallible {},
    };
    writer.finish()
}

/// Is this plane effectively black-and-white (a text/line-art scan)? Counts
/// midtone pixels on the luma channel against a small fraction of the total.
#[cfg(feature = "ccitt")]
fn plane_is_near_bilevel(plane: &DynamicImage) -> bool {
    let midtones;
    let total;
    match plane {
        DynamicImage::ImageLuma8(gray) => {
            total = gray.len();
            midtones = gray
                .pixels()
                .filter(|p| p[0] > BILEVEL_EXTREME_BAND && p[0] < 255 - BILEVEL_EXTREME_BAND)
                .count();
        }
        other => {
            // Only reachable when the caller skipped the pre-resize luma
            // conversion; compute luma on the fly for the check.
            let luma = other.to_luma8();
            total = luma.len();
            midtones = luma
                .pixels()
                .filter(|p| p[0] > BILEVEL_EXTREME_BAND && p[0] < 255 - BILEVEL_EXTREME_BAND)
                .count();
        }
    }
    (midtones as f64) <= (total as f64) * NEAR_BILEVEL_MIDTONE_FRACTION
}

/// G4 bytes for one probe round, memoized by target dimensions in the search
/// cache (G4 has no quality knob, so quality-only rounds are identical).
#[cfg(feature = "ccitt")]
fn g4_bytes_for_round(plane: &DynamicImage, cache: &mut Option<&mut ImageSearchCache>) -> Vec<u8> {
    let dims = (plane.width(), plane.height());
    if let Some((cached_dims, bytes)) = cache.as_ref().and_then(|c| c.bilevel_product.as_ref()) {
        if *cached_dims == dims {
            return bytes.clone();
        }
    }

    let bytes = match plane {
        DynamicImage::ImageLuma8(gray) => encode_gray_as_ccitt_g4(gray),
        other => {
            let luma = other.to_luma8();
            encode_gray_as_ccitt_g4(&luma)
        }
    };
    if let Some(cache) = cache.as_deref_mut() {
        cache.bilevel_product = Some((dims, bytes.clone()));
    }
    bytes
}

#[cfg(feature = "ccitt")]
fn optional_dict_integer(dict: &Dictionary, key: &[u8]) -> Option<i64> {
    match dict.get(key) {
        Ok(Object::Integer(value)) => Some(*value),
        _ => None,
    }
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
// Stream dictionary helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
struct StreamFilterInfo {
    has_jpeg: bool,
    has_ccitt: bool,
    has_jpx: bool,
    has_jbig2: bool,
    has_unsupported_filter: bool,
}

fn stream_filter_info(stream: &Stream) -> StreamFilterInfo {
    let mut names: Vec<Vec<u8>> = Vec::new();
    match stream.dict.get(b"Filter") {
        Ok(Object::Name(name)) => names.push(name.clone()),
        Ok(Object::Array(items)) => {
            for item in items {
                if let Object::Name(name) = item {
                    names.push(name.clone());
                }
            }
        }
        _ => {}
    }

    let mut info = StreamFilterInfo::default();
    for name in &names {
        match name.as_slice() {
            b"DCTDecode" => info.has_jpeg = true,
            b"CCITTFaxDecode" => info.has_ccitt = true,
            b"JPXDecode" => info.has_jpx = true,
            b"JBIG2Decode" => info.has_jbig2 = true,
            b"Crypt" => info.has_unsupported_filter = true,
            _ => {}
        }
    }

    // CCITT input is decodable in the pure G4 shape and the one-dimensional
    // G3 shapes (see `ccitt_input_shape`): the filter chain must be exactly
    // one CCITTFaxDecode (no flate pre-compression). Everything else — G3
    // two-dimensional coding, aligned EOL rows, flate mixes — stays
    // unsupported.
    #[cfg(feature = "ccitt")]
    let ccitt_decodable =
        info.has_ccitt && names.len() == 1 && ccitt_input_shape(stream).is_some();
    #[cfg(not(feature = "ccitt"))]
    let ccitt_decodable = false;
    if info.has_ccitt && !ccitt_decodable {
        info.has_unsupported_filter = true;
    }

    // JPX input is decodable when the feature is compiled in and the stream
    // dictionary passes the shape gate (plain dimensions, no DecodeParms /
    // Decode arrays; single-element filter chain). Without the feature every
    // JPX stream stays unsupported, keeping the default build pure Rust.
    #[cfg(feature = "jpx")]
    let jpx_decodable =
        info.has_jpx && names.len() == 1 && super::jpx::jpx_input_shape(stream).is_some();
    #[cfg(not(feature = "jpx"))]
    let jpx_decodable = false;
    if info.has_jpx && !jpx_decodable {
        info.has_unsupported_filter = true;
    }

    // JBIG2 input is decodable in the plain embedded shape: single-element
    // filter chain, sane dimensions, no DecodeParms/Decode arrays. The
    // `/JBIG2Globals` context is resolved document-side; everything here is
    // stream-local like the other gates.
    let jbig2_decodable =
        info.has_jbig2 && names.len() == 1 && super::jbig2::jbig2_input_shape(stream).is_some();
    if info.has_jbig2 && !jbig2_decodable {
        info.has_unsupported_filter = true;
    }

    info
}

pub(crate) fn longest_edge(stream: &Stream) -> Option<u32> {
    let w = optional_integer(stream, b"Width")?;
    let h = optional_integer(stream, b"Height")?;
    if w <= 0 || h <= 0 {
        return None;
    }
    Some(w.max(h) as u32)
}

/// Total pixel count from the stream dictionary; shared with the worker-pool
/// sizing in the compressor.
pub(super) fn pixel_count(stream: &Stream) -> Option<u64> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal JPEG byte stream carrying an SOF0 header with the given
    /// dimensions (values before the SOF marker are an APP0 segment).
    fn minimal_jpeg_with_dimensions(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x04, 0x41, 0x42]);
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        bytes
    }

    #[test]
    fn jpeg_header_reader_extracts_dimensions() {
        let jpeg = minimal_jpeg_with_dimensions(640, 480);
        assert_eq!(jpeg_dimensions_from_header(&jpeg), Some((640, 480)));
    }

    #[test]
    fn jpeg_header_reader_rejects_non_jpeg_input() {
        assert_eq!(jpeg_dimensions_from_header(&[0x25, 0x50, 0x44, 0x46]), None);
        assert_eq!(jpeg_dimensions_from_header(&[]), None);
        assert_eq!(jpeg_dimensions_from_header(&[0xFF, 0xD8]), None);
    }

    #[test]
    fn jpeg_header_reader_returns_none_for_zero_dimensions() {
        let jpeg = minimal_jpeg_with_dimensions(0, 0);
        assert_eq!(jpeg_dimensions_from_header(&jpeg), None);
    }

    #[test]
    fn resize_skips_images_within_tolerance() {
        let image = DynamicImage::ImageRgb8(image::RgbImage::new(1000, 500));
        let resized = resize_if_needed_fast(image, 1000);
        // 1000 <= 1000 * 1.08 tolerance → returned untouched.
        assert_eq!(resized.dimensions(), (1000, 500));
    }

    #[test]
    fn resize_downscales_longest_edge_to_target() {
        let image = DynamicImage::ImageRgb8(image::RgbImage::new(2000, 1000));
        let resized = resize_if_needed_fast(image, 1000);
        assert_eq!(resized.dimensions(), (1000, 500));
    }

    #[test]
    fn resize_never_produces_empty_images() {
        let image = DynamicImage::ImageRgb8(image::RgbImage::new(3, 2));
        let resized = resize_if_needed_fast(image, 8000);
        // Already within tolerance of the (huge) target — unchanged.
        assert_eq!(resized.dimensions(), (3, 2));
    }

    #[test]
    fn near_bilevel_detection_separates_scans_from_photographs() {
        use crate::testutil::{bilevel_scan_image, fixture_rgb_image};

        let scan = DynamicImage::ImageLuma8(bilevel_scan_image(400, 300, 7));
        assert!(plane_is_near_bilevel(&scan), "text scans are near-bilevel");

        let photo = DynamicImage::ImageRgb8(fixture_rgb_image(400, 300));
        assert!(
            !plane_is_near_bilevel(&photo),
            "photographs are continuous tone"
        );
    }

    #[test]
    #[cfg(feature = "ccitt")]
    fn g4_roundtrip_preserves_bilevel_pixels() {
        use crate::testutil::{bilevel_scan_image, encode_ccitt_g4};

        let image = bilevel_scan_image(331, 197, 11); // odd sizes on purpose
        let encoded = encode_ccitt_g4(&image);
        let (width, height) = image.dimensions();

        // Round-trip through the engine's own decoder, which carries the
        // polarity model this file documents (see `write_transitions_row`).
        let stream = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => width as i64,
                "Height" => height as i64,
                "BitsPerComponent" => 1,
                "Filter" => "CCITTFaxDecode",
                "DecodeParms" => dictionary! {
                    "K" => -1,
                    "Columns" => width as i64,
                    "Rows" => height as i64,
                    "BlackIs1" => true,
                },
            },
            encoded,
        );
        let decoded = decode_ccitt_stream(&stream).expect("fixture must decode cleanly");
        let DynamicImage::ImageLuma8(plane) = decoded else {
            panic!("CCITT decode must produce a luma plane");
        };
        assert_eq!(plane.as_raw(), image.as_raw());
    }

    #[test]
    #[cfg(feature = "ccitt")]
    fn g3_plain_roundtrip_preserves_bilevel_pixels() {
        use crate::testutil::{bilevel_scan_image, encode_ccitt_g3_1d};

        // The second case mirrors the integration fixture exactly (full fax
        // page width; larger runs exercise multi-makeup sequences).
        for ((width, height, seed), (byte_align, with_eol)) in [
            ((331u32, 37u32, 11u32), (false, false)),
            ((331, 37, 11), (true, false)),
            ((331, 37, 11), (false, true)),
            ((1728, 1200, 0x1234_5678), (false, false)),
            ((1728, 1200, 0x1234_5678), (true, false)),
            ((1728, 1200, 0x1234_5678), (false, true)),
        ] {
            let image = bilevel_scan_image(width, height, seed);
            let encoded = encode_ccitt_g3_1d(&image, byte_align, with_eol);
            let (width, height) = image.dimensions();

            let stream = Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Image",
                    "Width" => width as i64,
                    "Height" => height as i64,
                    "BitsPerComponent" => 1,
                    "Filter" => "CCITTFaxDecode",
                    "DecodeParms" => dictionary! {
                        "K" => 0,
                        "Columns" => width as i64,
                        "Rows" => height as i64,
                        "BlackIs1" => true,
                        "EncodedByteAlign" => byte_align,
                        "EndOfLine" => with_eol,
                    },
                },
                encoded,
            );
            let decoded = decode_ccitt_stream(&stream)
                .unwrap_or_else(|error| panic!("align={byte_align} eol={with_eol}: {error}"));
            let DynamicImage::ImageLuma8(plane) = decoded else {
                panic!("CCITT decode must produce a luma plane");
            };
            assert_eq!(
                plane.as_raw(),
                image.as_raw(),
                "align={byte_align} eol={with_eol} must round-trip"
            );
        }
    }

    #[test]
    fn detail_score_separates_flat_from_detailed_planes() {
        use crate::testutil::{fixture_rgb_image, gradient_rgb_image};

        let flat = DynamicImage::ImageRgb8(gradient_rgb_image(400, 300, FIXTURE_SEED_FOR_TESTS));
        let detailed = DynamicImage::ImageRgb8(fixture_rgb_image(400, 300));

        let flat_score = plane_detail_score(&flat);
        let detailed_score = plane_detail_score(&detailed);
        assert!(flat_score < DETAIL_LOW_SCORE, "gradient is flat: {flat_score}");
        assert!(
            detailed_score > DETAIL_HIGH_SCORE,
            "noisy fixture is detailed: {detailed_score}"
        );
    }

    #[test]
    fn search_round_quality_offsets_and_clamps() {
        use crate::testutil::{fixture_rgb_image, gradient_rgb_image};

        let flat = DynamicImage::ImageRgb8(gradient_rgb_image(400, 300, FIXTURE_SEED_FOR_TESTS));
        let detailed = DynamicImage::ImageRgb8(fixture_rgb_image(400, 300));

        assert_eq!(search_round_quality(72, &flat), 60, "flat planes get the penalty");
        assert_eq!(
            search_round_quality(72, &detailed),
            78,
            "detailed planes get the boost"
        );
        // Clamping at both ends of the encoder's quality range.
        assert_eq!(search_round_quality(15, &flat), 10);
        assert_eq!(search_round_quality(99, &detailed), 100);
    }

    /// A 64×32 reference G4 bitstream (white background, black bar at rows
    /// 12..=20 / cols 20..=44), generated with the battle-tested `tiffcp
    /// -c g4`. T.4 polarity: 0 bits are white.
    const REFERENCE_G4_BAR: &[u8] = &[
        0x26, 0xA0, 0x78, 0x6F, 0xFF, 0xFF, 0xFC, 0x86, 0x85, 0x7F, 0xFF, 0xFF, 0xFF, 0xF8,
        0xFF, 0xFF, 0xFC, 0x00, 0x40, 0x04,
    ];

    fn reference_bar_plane() -> image::GrayImage {
        let mut image = image::GrayImage::from_pixel(64, 32, image::Luma([255u8]));
        for y in 12..=20 {
            for x in 20..=44 {
                image.put_pixel(x, y, image::Luma([0u8]));
            }
        }
        image
    }

    #[test]
    #[cfg(feature = "ccitt")]
    fn g4_decode_matches_reference_bitstream() {
        let stream = Stream::new(
            dictionary! {
                "Width" => 64,
                "Height" => 32,
                "BitsPerComponent" => 1,
                "Filter" => "CCITTFaxDecode",
                "DecodeParms" => dictionary! {
                    "K" => -1, "Columns" => 64, "Rows" => 32, "BlackIs1" => true,
                },
            },
            REFERENCE_G4_BAR.to_vec(),
        );
        let decoded = decode_ccitt_stream(&stream).expect("reference stream decodes");
        let DynamicImage::ImageLuma8(plane) = decoded else {
            panic!("must decode to luma");
        };
        assert_eq!(plane.as_raw(), reference_bar_plane().as_raw());
    }

    #[test]
    #[cfg(feature = "ccitt")]
    fn g4_output_renders_white_background_as_white() {
        // The polarity end-to-end gate: embed the engine-encoded G4 with the
        // exact dict shape the compressor writes and check an independent
        // renderer (poppler) keeps white as white. Skipped when pdftoppm is
        // unavailable; the byte-level guarantees are covered by the
        // reference-bitstream tests above.
        let scratch = std::env::temp_dir().join("pdf-core-g4-render.pdf");
        if std::process::Command::new("pdftoppm")
            .arg("-h")
            .output()
            .is_err()
        {
            eprintln!("pdftoppm unavailable — skipping the G4 render gate");
            return;
        }

        let g4 = encode_gray_as_ccitt_g4(&reference_bar_plane());
        let mut pdf = format!(
            "%PDF-1.5\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
             3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 64 32]/Contents 5 0 R/Resources<</XObject<</Im0 4 0 R>>>>>>endobj\n\
             4 0 obj<</Type/XObject/Subtype/Image/Width 64/Height 32/ColorSpace/DeviceGray/BitsPerComponent 1/Filter/CCITTFaxDecode/DecodeParms<</K -1/Columns 64/Rows 32/BlackIs1 true>>/Length {}>>stream\n",
            g4.len(),
        )
        .into_bytes();
        pdf.extend_from_slice(&g4);
        pdf.extend_from_slice(b"\nendstream\nendobj\n5 0 obj<</Length 22>>stream\nq 64 0 0 32 0 0 cm /Im0 Do Q\nendstream\nendobj\n");
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n0000000009 00000 n \n0000000058 00000 n \n0000000111 00000 n \n");
        pdf.extend_from_slice(format!("{:010} 00000 n \n", 300).as_bytes());
        pdf.extend_from_slice(b"0000000487 00000 n \ntrailer<</Size 6/Root 1 0 R>>\nstartxref\n");
        pdf.extend_from_slice(xref.to_string().as_bytes());
        pdf.extend_from_slice(b"\n%%EOF");
        std::fs::write(&scratch, &pdf).unwrap();

        let prefix = std::env::temp_dir().join("pdf-core-g4-render");
        let rendered = std::process::Command::new("pdftoppm")
            .args(["-r", "30", "-png", "-singlefile"])
            .arg(&scratch)
            .arg(&prefix)
            .output()
            .expect("pdftoppm runs");
        assert!(
            rendered.status.success(),
            "{}",
            String::from_utf8_lossy(&rendered.stderr)
        );
        let png = std::fs::read(prefix.with_extension("png")).unwrap();
        let raster = image::load_from_memory(&png).unwrap().to_luma8();
        let (w, h) = raster.dimensions();
        let corner = raster.get_pixel(1, 1)[0];
        let bar = raster.get_pixel(w / 2, h / 2)[0];
        println!("engine-encoded G4 rendered: corner={corner} bar={bar}");
        assert!(
            corner > 200 && bar < 60,
            "engine G4 must render white bg + black bar"
        );
    }

    const FIXTURE_SEED_FOR_TESTS: u32 = 0x1234_5678;
}
