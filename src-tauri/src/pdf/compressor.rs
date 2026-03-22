use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    time::Instant,
};

use image::{codecs::jpeg::JpegEncoder, imageops::FilterType, DynamicImage, GenericImageView};
use lopdf::{Document, Object, ObjectId, Stream};

use crate::{error::AppError, models::CompressionResponse};

use super::settings::CompressionSettings;

#[derive(Debug, Default)]
struct CompressionStats {
    images_recompressed: usize,
    images_skipped: usize,
    streams_compressed: usize,
    metadata_removed: bool,
    warnings: Vec<String>,
    notes: Vec<String>,
}

/// Compresses an existing PDF in place at the object level.
///
/// The optimizer preserves page structure, text, and vector content. It only
/// rewrites safe image streams, compresses eligible non-image streams, and can
/// optionally trim document metadata.
pub fn compress_pdf(
    path: &str,
    settings: CompressionSettings,
) -> Result<CompressionResponse, AppError> {
    let started_at = Instant::now();
    let input_path = validate_input_path(path)?;
    let original_size_bytes = fs::metadata(&input_path)?.len();
    let output_path = build_output_path(&input_path, settings)?;

    let mut document = Document::load(&input_path)
        .map_err(|error| AppError::PdfBuild(format!("Failed to load PDF: {error}")))?;
    let mut stats = CompressionStats::default();

    optimize_document(&mut document, settings, &mut stats)?;

    document.prune_objects();
    document.renumber_objects();

    let mut output_file = fs::File::create(&output_path)?;
    document
        .save_modern(&mut output_file)
        .map_err(|error| AppError::PdfBuild(format!("Failed to save optimized PDF: {error}")))?;

    let compressed_size_bytes = fs::metadata(&output_path)?.len();
    let saved_bytes = original_size_bytes.saturating_sub(compressed_size_bytes);
    let savings_percent = if original_size_bytes == 0 {
        0.0
    } else {
        (saved_bytes as f32 / original_size_bytes as f32) * 100.0
    };

    stats.notes.insert(
        0,
        format!(
            "Applied the '{}' profile with JPEG quality {} and max image edge {} px.",
            settings.preset.as_label(),
            settings.image_quality,
            settings.max_image_size_px
        ),
    );
    stats.notes.push(
        "The optimizer preserves text and vector instructions whenever it cannot safely rewrite a given object."
            .to_string(),
    );
    if !stats.metadata_removed {
        stats.notes.push(
            "Document metadata was left in place because metadata cleanup was disabled or unavailable."
                .to_string(),
        );
    }

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
        warnings: stats.warnings,
        notes: stats.notes,
    })
}

fn optimize_document(
    document: &mut Document,
    settings: CompressionSettings,
    stats: &mut CompressionStats,
) -> Result<(), AppError> {
    let object_ids: Vec<ObjectId> = document.objects.keys().copied().collect();

    for object_id in object_ids {
        let Some(object) = document.objects.get_mut(&object_id) else {
            continue;
        };

        let Object::Stream(stream) = object else {
            continue;
        };

        // Image streams are the highest-value optimization target, but they are
        // also the easiest place to damage fidelity, so they take the stricter path.
        if is_image_stream(stream) {
            if settings.optimize_images {
                match optimize_image_stream(stream, settings)? {
                    ImageOptimization::Recompressed => stats.images_recompressed += 1,
                    ImageOptimization::Skipped(reason) => {
                        stats.images_skipped += 1;
                        stats
                            .warnings
                            .push(format!("Skipped image object {:?}: {reason}", object_id));
                    }
                }
            }
            continue;
        }

        if settings.compress_streams && compress_non_image_stream(stream) {
            stats.streams_compressed += 1;
        }
    }

    if settings.strip_metadata {
        stats.metadata_removed = remove_metadata(document);
    }

    Ok(())
}

fn compress_non_image_stream(stream: &mut Stream) -> bool {
    if stream.is_compressed() || !stream.allows_compression {
        return false;
    }

    stream.compress().is_ok()
}

enum ImageOptimization {
    Recompressed,
    Skipped(String),
}

fn optimize_image_stream(
    stream: &mut Stream,
    settings: CompressionSettings,
) -> Result<ImageOptimization, AppError> {
    // Skip masked images until there is a safe rewrite path for transparency
    // and image masks; flattening them here would risk visible corruption.
    if has_mask(stream) {
        return Ok(ImageOptimization::Skipped(
            "transparency or image masks are not rewritten yet".to_string(),
        ));
    }

    let filter_names = stream_filter_names(stream);
    // Keep unsupported or high-risk encoded images untouched rather than trying
    // to decode every filter combination and possibly damage the output PDF.
    if filter_names.iter().any(|filter| {
        matches!(
            filter.as_str(),
            "JPXDecode" | "JBIG2Decode" | "CCITTFaxDecode" | "Crypt"
        )
    }) {
        return Ok(ImageOptimization::Skipped(
            "unsupported image filter for safe recompression".to_string(),
        ));
    }

    let dynamic_image = if filter_names.iter().any(|filter| filter == "DCTDecode") {
        image::load_from_memory(&stream.content).map_err(|error| {
            AppError::PdfBuild(format!("Failed to decode JPEG image stream: {error}"))
        })?
    } else {
        decode_raw_image_stream(stream)?
    };

    let optimized = resize_if_needed(dynamic_image, settings.max_image_size_px);
    let color_space_name = if optimized.color().has_color() {
        "DeviceRGB"
    } else {
        "DeviceGray"
    };
    let encoded = encode_dynamic_image_as_jpeg(&optimized, settings.image_quality)?;

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

    Ok(ImageOptimization::Recompressed)
}

fn resize_if_needed(image: DynamicImage, max_edge: u16) -> DynamicImage {
    let (width, height) = image.dimensions();
    let longest_edge = width.max(height);
    let max_edge = u32::from(max_edge);

    if longest_edge <= max_edge {
        return image;
    }

    let scale = max_edge as f32 / longest_edge as f32;
    let target_width = ((width as f32) * scale).round().max(1.0) as u32;
    let target_height = ((height as f32) * scale).round().max(1.0) as u32;

    image.resize(target_width, target_height, FilterType::Lanczos3)
}

fn decode_raw_image_stream(stream: &Stream) -> Result<DynamicImage, AppError> {
    let width = required_integer(stream, b"Width")? as u32;
    let height = required_integer(stream, b"Height")? as u32;
    let bits_per_component = optional_integer(stream, b"BitsPerComponent").unwrap_or(8);
    let color_space =
        optional_name(stream, b"ColorSpace").unwrap_or_else(|| "DeviceRGB".to_string());

    if bits_per_component != 8 {
        return Err(AppError::PdfBuild(
            "Only 8-bit embedded raw images are currently supported for recompression.".to_string(),
        ));
    }

    let decoded = stream.decompressed_content().map_err(|error| {
        AppError::PdfBuild(format!("Failed to decompress raw image stream: {error}"))
    })?;

    match color_space.as_str() {
        "DeviceGray" => image::GrayImage::from_raw(width, height, decoded)
            .map(DynamicImage::ImageLuma8)
            .ok_or_else(|| {
                AppError::PdfBuild(
                    "Embedded grayscale image bytes did not match the declared dimensions."
                        .to_string(),
                )
            }),
        "DeviceRGB" => image::RgbImage::from_raw(width, height, decoded)
            .map(DynamicImage::ImageRgb8)
            .ok_or_else(|| {
                AppError::PdfBuild(
                    "Embedded RGB image bytes did not match the declared dimensions.".to_string(),
                )
            }),
        other => Err(AppError::PdfBuild(format!(
            "Unsupported embedded image color space for recompression: {other}"
        ))),
    }
}

fn encode_dynamic_image_as_jpeg(image: &DynamicImage, quality: u8) -> Result<Vec<u8>, AppError> {
    let mut cursor = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut cursor, quality);
    encoder.encode_image(image)?;
    Ok(cursor.into_inner())
}

fn is_image_stream(stream: &Stream) -> bool {
    matches!(optional_name(stream, b"Subtype").as_deref(), Some("Image"))
}

fn has_mask(stream: &Stream) -> bool {
    stream.dict.get(b"SMask").is_ok() || stream.dict.get(b"Mask").is_ok()
}

fn stream_filter_names(stream: &Stream) -> Vec<String> {
    match stream.dict.get(b"Filter") {
        Ok(Object::Name(name)) => vec![String::from_utf8_lossy(name).into_owned()],
        Ok(Object::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                Object::Name(name) => Some(String::from_utf8_lossy(name).into_owned()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
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
            "The embedded image stream is missing a required integer dictionary key: {}",
            String::from_utf8_lossy(key)
        ))
    })
}

fn optional_integer(stream: &Stream, key: &[u8]) -> Option<i64> {
    match stream.dict.get(key) {
        Ok(Object::Integer(value)) => Some(*value),
        Ok(Object::Real(value)) => Some(*value as i64),
        _ => None,
    }
}

fn remove_metadata(document: &mut Document) -> bool {
    let mut removed_any = false;

    if document.trailer.remove(b"Info").is_some() {
        removed_any = true;
    }

    let root_reference = document
        .trailer
        .get(b"Root")
        .ok()
        .and_then(|object| object.as_reference().ok());

    if let Some(root_reference) = root_reference {
        let metadata_reference = document
            .objects
            .get(&root_reference)
            .and_then(|object| object.as_dict().ok())
            .and_then(|dict| dict.get(b"Metadata").ok())
            .and_then(|object| object.as_reference().ok());

        if let Some(metadata_reference) = metadata_reference {
            document.objects.remove(&metadata_reference);
            removed_any = true;
        }

        if let Some(Object::Dictionary(root_dict)) = document.objects.get_mut(&root_reference) {
            if root_dict.remove(b"Metadata").is_some() {
                removed_any = true;
            }
        }
    }

    removed_any
}

fn build_output_path(
    input_path: &Path,
    settings: CompressionSettings,
) -> Result<PathBuf, AppError> {
    let parent = input_path.parent().ok_or_else(|| {
        AppError::PdfBuild(
            "Could not determine the source directory for the output PDF.".to_string(),
        )
    })?;
    let stem = input_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| AppError::PdfBuild("Could not read the source filename.".to_string()))?;

    let base_name = format!("{stem}__optimized-{}", settings.preset.as_label());

    // Never overwrite the source file. If the preferred output name already
    // exists, keep appending a numeric suffix until a free path is found.
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
        "Could not find a free output filename after many attempts.".to_string(),
    ))
}

fn validate_input_path(path: &str) -> Result<PathBuf, AppError> {
    let candidate = PathBuf::from(path);

    if !candidate.exists() {
        return Err(AppError::MissingInput(candidate));
    }

    let is_pdf = candidate
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));

    if !is_pdf {
        return Err(AppError::InvalidPdfPath(candidate));
    }

    Ok(candidate)
}
