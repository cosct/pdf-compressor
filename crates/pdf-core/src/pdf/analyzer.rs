//! Preflight PDF analysis engine — heuristic-based document classification.
//! PDF 预检分析引擎 — 基于启发式的文档分类。
//!
//! Inspects page structure, text density, image signals, and font resources
//! to classify documents as text-native, mixed, or scan-heavy, and recommends
//! a compression preset with estimated savings.
//! 检查页面结构、文本密度、图片信号和字体资源，
//! 将文档分类为 text-native、mixed 或 scan-heavy，并推荐压缩预设和预估节省比例。

use std::{
    collections::{BTreeMap, HashSet},
    fs,
};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::{
    error::AppError,
    models::{AnalysisResponse, BackendNotice, ProgressUpdate},
    pdf::encode::SkipPolicy,
    pdf::settings::CompressionPreset,
};

use super::compressor::ensure_input_size_supported;
use super::{optional_integer, validate_input_path};

const TEXT_NATIVE_KIND: &str = "text-native";
const MIXED_KIND: &str = "mixed";
const SCAN_HEAVY_KIND: &str = "scan-heavy";
const MAX_ANALYSIS_SAMPLE_PAGES: usize = 24;

#[derive(Debug, Default)]
struct AnalysisSignals {
    inspected_pages: usize,
    extracted_text_characters: usize,
    pages_with_extractable_text: usize,
    pages_with_text_showing_ops: usize,
    pages_with_font_resources: usize,
    pages_with_images: usize,
    total_page_image_references: usize,
    text_extraction_failures: usize,
}

#[derive(Debug, Default)]
struct ImageDimensionStats {
    image_object_count: usize,
    longest_edges: Vec<u32>,
    /// Total stored bytes of all image streams.
    total_image_bytes: u64,
    /// Bytes in images the compressor can plausibly act on — codec supported
    /// and not skipped by the fast-path heuristics at the recommended edge.
    /// The basis for the savings estimate; over-counting here is exactly how
    /// "estimated 36%, delivered 1%" happens (e.g. CMYK-heavy print files).
    actionable_image_bytes: u64,
    /// Images stored with codecs (Crypt, or CCITT/JPX/JBIG2 variants outside
    /// the supported decode shapes) that no safe re-encode path exists for;
    /// they are preserved untouched.
    unsupported_codec_count: usize,
}

/// One image XObject observed during the object scan.
struct ImageStreamRecord {
    bytes: u64,
    longest_edge: Option<u32>,
    pixels: Option<u64>,
    is_jpeg: bool,
    codec_supported: bool,
    /// The stream declares CMYK (`/DeviceCMYK` name — analyzer-side proof;
    /// ICC N=4 and CMYK palettes need the compressor's full resolution).
    cmyk_declared: bool,
}

/// Analyze a PDF without producing any output: classify the document
/// (text-native / mixed / scan-heavy), estimate achievable savings, and
/// recommend a preset. Samples up to 24 pages for the text/image signals and
/// mirrors the compressor's skip heuristics so the estimate only counts
/// images the compressor can actually act on.
///
/// `password` unlocks open-password-encrypted files; see
/// [`crate::pdf::compress_pdf_with_progress`] for the encrypted-input matrix.
/// 分析 PDF：分类文档、估算可节省比例并推荐预设，不产生任何输出。
pub fn analyze_pdf_with_progress<F>(
    path: &str,
    password: Option<&str>,
    mut report_progress: F,
) -> Result<AnalysisResponse, AppError>
where
    F: FnMut(ProgressUpdate),
{
    report_progress(ProgressUpdate::new("analyzing", 5.0));

    let input_path = validate_input_path(path)?;
    let file_size_bytes = fs::metadata(&input_path)?.len();
    ensure_input_size_supported(file_size_bytes)?;

    let password_attempted = password.is_some_and(|value| !value.is_empty());
    let mut document = super::load_document(&input_path, password).map_err(|error| {
        // Password and encryption errors carry their own UI-facing codes —
        // wrapping them in PdfBuild would hide the GUI's password-retry
        // entry point after a wrong first attempt.
        match error {
            AppError::WrongPassword
            | AppError::PasswordRequired
            | AppError::Encrypted
            | AppError::MissingInput(_)
            | AppError::InvalidPdfPath(_) => error,
            other => AppError::PdfBuild(format!("Failed to inspect PDF structure: {other}")),
        }
    })?;
    let decrypted_with_empty_password =
        super::ensure_not_encrypted(&mut document, password_attempted)?;

    report_progress(ProgressUpdate::new("analyzing", 20.0));

    let page_map = document.get_pages();
    let page_count = page_map.len();
    let image_records = collect_image_stream_records(&document);
    let image_object_count = image_records.len();
    let longest_edges: Vec<u32> = image_records
        .iter()
        .filter_map(|record| record.longest_edge)
        .collect();

    report_progress(ProgressUpdate::new("analyzing", 45.0));

    let signals = collect_analysis_signals(&document, &page_map);
    let inspected_page_count = signals.inspected_pages.max(1);

    report_progress(ProgressUpdate::new("analyzing", 80.0));

    let average_bytes_per_page = average_per_page(file_size_bytes as f32, page_count);
    let average_text_per_page = average_per_page(
        signals.extracted_text_characters as f32,
        inspected_page_count,
    );
    let average_images_per_page = if signals.total_page_image_references > 0 {
        average_per_page(
            signals.total_page_image_references as f32,
            inspected_page_count,
        )
    } else {
        average_per_page(image_object_count as f32, page_count)
    };
    let text_page_ratio = ratio(signals.pages_with_extractable_text, inspected_page_count);
    let structural_text_ratio = ratio(
        signals
            .pages_with_extractable_text
            .max(signals.pages_with_text_showing_ops)
            .max(signals.pages_with_font_resources),
        inspected_page_count,
    );
    let image_page_ratio = ratio(signals.pages_with_images, inspected_page_count);
    let extraction_uncertainty_ratio =
        ratio(signals.text_extraction_failures, inspected_page_count);

    let image_coverage = ((average_bytes_per_page / 180_000.0).min(1.0) * 45.0
        + image_page_ratio * 35.0
        + (average_images_per_page.min(3.0) / 3.0) * 20.0
        - structural_text_ratio * 10.0)
        .clamp(8.0, 99.0);
    let scanned_confidence = (((140.0 - average_text_per_page.min(140.0)) / 140.0) * 45.0
        + image_page_ratio * 25.0
        + (average_images_per_page.min(2.5) / 2.5) * 15.0
        + (average_bytes_per_page / 240_000.0).min(1.0) * 25.0
        - structural_text_ratio * 30.0
        - extraction_uncertainty_ratio * 12.0)
        .clamp(5.0, 99.0);

    let has_strong_text_signal = average_text_per_page >= 120.0 && structural_text_ratio >= 0.6;
    let is_likely_scanned = scanned_confidence >= 60.0
        && average_text_per_page < 110.0
        && image_page_ratio >= 0.35
        && structural_text_ratio < 0.75;
    let document_kind =
        if has_strong_text_signal && image_page_ratio < 0.65 && average_images_per_page < 1.25 {
            TEXT_NATIVE_KIND
        } else if is_likely_scanned {
            SCAN_HEAVY_KIND
        } else {
            MIXED_KIND
        };

    let recommended_preset = if scanned_confidence >= 80.0 {
        CompressionPreset::Maximum
    } else if scanned_confidence >= 48.0 {
        CompressionPreset::Balanced
    } else {
        CompressionPreset::Conservative
    };

    let recommended_max_image_size_px =
        recommend_max_image_size_px(recommended_preset, &longest_edges);
    // Fold the raw records into stats under the recommended edge so the
    // estimate mirrors the compressor's skip heuristics.
    let image_stats =
        summarize_image_records(image_records, u32::from(recommended_max_image_size_px));

    let estimated_savings_percent = estimate_savings_percent(
        recommended_preset,
        image_stats.actionable_image_bytes,
        file_size_bytes,
    );

    let recommended_image_quality = recommended_preset.default_quality();
    let max_image_edge_px = if image_stats.longest_edges.is_empty() {
        0
    } else {
        *image_stats.longest_edges.iter().max().unwrap_or(&0) as u16
    };

    let mut notices = vec![
        BackendNotice::new(
            "analysis.note.structureBased",
            "neutral",
            "Analysis uses PDF structure inspection instead of page rendering.",
        ),
        BackendNotice::new(
            "analysis.note.safeOptimization",
            "neutral",
            "Compression focuses on image streams, metadata, and compressible PDF streams.",
        ),
    ];

    if inspected_page_count < page_count {
        notices.push(
            BackendNotice::new(
                "analysis.note.sampledPages",
                "neutral",
                format!(
                    "Large PDF detected, so detailed page inspection sampled {inspected_page_count} of {page_count} pages."
                ),
            )
            .with_value("inspectedPages", inspected_page_count.to_string())
            .with_value("pageCount", page_count.to_string()),
        );
    }

    if signals.text_extraction_failures > 0 {
        notices.push(BackendNotice::new(
            "analysis.warning.textExtractionFallback",
            "warning",
            "Some pages could not be fully decoded, so the recommendation stays conservative.",
        ));
    }

    if document_kind == TEXT_NATIVE_KIND {
        notices.push(BackendNotice::new(
            "analysis.warning.textNative",
            "warning",
            "This file looks mostly text-native, so savings may stay modest.",
        ));
    }

    if page_count > 150 {
        notices.push(BackendNotice::new(
            "analysis.warning.largePageCount",
            "warning",
            "This PDF has many pages, so preparation and compression may take longer.",
        ));
    }

    if image_stats.image_object_count == 0 {
        notices.push(BackendNotice::new(
            "analysis.warning.noImages",
            "warning",
            "No embedded image objects were detected, so savings may rely on stream compression and metadata cleanup.",
        ));
    }

    if image_stats.unsupported_codec_count > 0 {
        notices.push(
            BackendNotice::new(
                "analysis.warning.unsupportedImageCodecs",
                "warning",
                format!(
                    "{} images use codecs (Crypt, or CCITT/JPX/JBIG2 variants outside the \
                     supported decode shapes) that this version cannot re-encode; they are \
                     preserved as-is and excluded from the estimate.",
                    image_stats.unsupported_codec_count
                ),
            )
            .with_value("count", image_stats.unsupported_codec_count.to_string()),
        );
    }

    if decrypted_with_empty_password {
        notices.push(BackendNotice::new(
            "analysis.note.encryptedUnlocked",
            "neutral",
            "This PDF used owner-password encryption and was unlocked with the empty \
             user password; the compressed export will be unencrypted.",
        ));
    }

    if document_kind == MIXED_KIND && text_page_ratio > 0.25 && image_page_ratio > 0.25 {
        notices.push(BackendNotice::new(
            "analysis.note.mixedDocument",
            "neutral",
            "This PDF mixes readable text structure with image-heavy pages, so the recommendation favors a safer first pass.",
        ));
    }

    if file_size_bytes < 1_000_000 {
        notices.push(BackendNotice::new(
            "analysis.note.smallPdf",
            "neutral",
            "Small PDFs often have less room to shrink dramatically.",
        ));
    }

    report_progress(
        ProgressUpdate::new("done", 100.0).with_message(BackendNotice::new(
            "analysis.progress.done",
            "success",
            "Preparation finished.",
        )),
    );

    Ok(AnalysisResponse {
        file_size_bytes: file_size_bytes as f64,
        page_count: page_count as u32,
        image_object_count: image_stats.image_object_count as u32,
        document_kind: document_kind.to_string(),
        scanned_confidence,
        image_coverage,
        estimated_savings_percent,
        recommended_preset: recommended_preset.as_label().to_string(),
        max_image_edge_px,
        recommended_max_image_size_px,
        recommended_image_quality,
        is_likely_scanned,
        notices,
    })
}

fn collect_analysis_signals(
    document: &Document,
    page_map: &BTreeMap<u32, ObjectId>,
) -> AnalysisSignals {
    let mut signals = AnalysisSignals::default();
    let sampled_pages = build_analysis_page_sample(page_map);
    signals.inspected_pages = sampled_pages.len();

    for (page_number, page_id) in sampled_pages {
        let (page_text_characters, had_text_extraction_error) =
            extract_page_text_characters(document, page_number);
        let (page_image_references, direct_image_references) =
            collect_page_image_references(document, page_id);
        let has_images = !page_image_references.is_empty() || direct_image_references > 0;
        // Extractable characters imply text-showing operators exist — skip
        // the second content decode for pages where extraction already
        // produced text.
        let has_text_showing_ops =
            page_text_characters > 0 || page_has_text_showing_operations(document, page_id);
        let has_font_resources = page_has_font_resources(document, page_id);

        signals.extracted_text_characters += page_text_characters;
        signals.total_page_image_references +=
            page_image_references.len() + direct_image_references;

        if page_text_characters > 0 {
            signals.pages_with_extractable_text += 1;
        }

        if had_text_extraction_error {
            signals.text_extraction_failures += 1;
        }

        if has_images {
            signals.pages_with_images += 1;
        }

        if has_text_showing_ops {
            signals.pages_with_text_showing_ops += 1;
        }

        if has_font_resources {
            signals.pages_with_font_resources += 1;
        }
    }

    signals
}

fn build_analysis_page_sample(page_map: &BTreeMap<u32, ObjectId>) -> Vec<(u32, ObjectId)> {
    let pages: Vec<(u32, ObjectId)> = page_map
        .iter()
        .map(|(&page_number, &page_id)| (page_number, page_id))
        .collect();

    if pages.len() <= MAX_ANALYSIS_SAMPLE_PAGES {
        return pages;
    }

    let sample_slots = MAX_ANALYSIS_SAMPLE_PAGES.saturating_sub(1).max(1);
    let last_index = pages.len().saturating_sub(1);
    let mut sampled_indexes = HashSet::new();

    for slot in 0..MAX_ANALYSIS_SAMPLE_PAGES {
        let ratio = slot as f32 / sample_slots as f32;
        let index = ((last_index as f32) * ratio).round() as usize;
        sampled_indexes.insert(index.min(last_index));
    }

    let mut sampled_indexes: Vec<usize> = sampled_indexes.into_iter().collect();
    sampled_indexes.sort_unstable();
    sampled_indexes
        .into_iter()
        .map(|index| pages[index])
        .collect()
}

/// Scan every image XObject in the document into raw records.
fn collect_image_stream_records(document: &Document) -> Vec<ImageStreamRecord> {
    let mut records = Vec::new();

    for object in document.objects.values() {
        let Object::Stream(stream) = object else {
            continue;
        };

        if !stream_has_subtype(stream, b"Image") {
            continue;
        }

        let width = optional_integer(stream, b"Width").filter(|value| *value > 0);
        let height = optional_integer(stream, b"Height").filter(|value| *value > 0);
        let (longest_edge, pixels) = match (width, height) {
            (Some(width), Some(height)) => {
                let (w, h) = (width as u32, height as u32);
                (Some(w.max(h)), Some(u64::from(w) * u64::from(h)))
            }
            _ => (None, None),
        };
        let (is_jpeg, codec_supported) = super::encode::image_codec_class(stream);
        let cmyk_declared = matches!(
            stream.dict.get(b"ColorSpace"),
            Ok(Object::Name(name)) if name.as_slice() == b"DeviceCMYK"
        );

        records.push(ImageStreamRecord {
            bytes: stream.content.len() as u64,
            longest_edge,
            pixels,
            is_jpeg,
            codec_supported,
            cmyk_declared,
        });
    }

    records
}

/// Fold raw records into stats under the recommended preset's edge and the
/// document-shaped skip policy.
fn summarize_image_records(
    records: Vec<ImageStreamRecord>,
    recommended_edge: u32,
) -> ImageDimensionStats {
    let mut stats = ImageDimensionStats {
        image_object_count: records.len(),
        ..ImageDimensionStats::default()
    };
    let skip_policy = SkipPolicy::for_document(stats.image_object_count, false);
    for record in &records {
        stats.total_image_bytes += record.bytes;
        if let Some(edge) = record.longest_edge {
            stats.longest_edges.push(edge);
        }
        if !record.codec_supported {
            stats.unsupported_codec_count += 1;
            continue;
        }
        if super::encode::image_is_actionable(
            record.is_jpeg,
            record.codec_supported,
            record.bytes,
            record.longest_edge,
            record.pixels,
            recommended_edge,
            skip_policy,
            // The analyzer runs settings-free, so it mirrors the default
            // (CMYK conversion off): declared-CMYK images are excluded from
            // the savings estimate, erring low rather than promising shifts.
            record.cmyk_declared,
        ) {
            stats.actionable_image_bytes += record.bytes;
        }
    }

    stats
}

/// Estimate savings from what actually shrinks: the **actionable** image bytes
/// (codec supported and not skipped by the fast-path heuristics) re-encoded at
/// the preset's quality/edge targets, plus a small flat gain from stream
/// compression and metadata cleanup of the non-image remainder. Falls back to
/// a modest floor for image-free documents.
fn estimate_savings_percent(
    preset: CompressionPreset,
    total_image_bytes: u64,
    file_size_bytes: u64,
) -> f32 {
    if file_size_bytes == 0 {
        return 5.0;
    }

    // Empirical post-recompression ratios for photographic content.
    let image_ratio = match preset {
        CompressionPreset::Conservative => 0.62,
        CompressionPreset::Balanced => 0.48,
        CompressionPreset::Maximum => 0.34,
    };
    let non_image_bytes = file_size_bytes.saturating_sub(total_image_bytes);

    let image_gain = total_image_bytes as f32 * (1.0 - image_ratio);
    let stream_gain = non_image_bytes as f32 * 0.05;
    let savings = (image_gain + stream_gain) / file_size_bytes as f32 * 100.0;

    savings.clamp(3.0, 85.0)
}

fn recommend_max_image_size_px(preset: CompressionPreset, longest_edges: &[u32]) -> u16 {
    if longest_edges.is_empty() {
        return preset.default_max_image_size_px();
    }

    let mut sorted = longest_edges.to_vec();
    sorted.sort_unstable();

    let index = ((sorted.len().saturating_sub(1)) as f32 * 0.75).round() as usize;
    let base_edge = sorted[index].clamp(800, 3200) as f32;
    let scaled = match preset {
        CompressionPreset::Conservative => base_edge,
        CompressionPreset::Balanced => base_edge * 0.85,
        CompressionPreset::Maximum => base_edge * 0.7,
    };

    let rounded = ((scaled / 100.0).round() * 100.0).clamp(800.0, 3200.0);
    rounded as u16
}

fn extract_page_text_characters(document: &Document, page_number: u32) -> (usize, bool) {
    let mut character_count = 0usize;
    let mut had_error = false;

    for chunk in document.extract_text_chunks(&[page_number]) {
        match chunk {
            Ok(text) => {
                character_count += text
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .count();
            }
            Err(_) => had_error = true,
        }
    }

    (character_count, had_error)
}

fn collect_page_image_references(
    document: &Document,
    page_id: ObjectId,
) -> (HashSet<ObjectId>, usize) {
    let mut image_ids = HashSet::new();
    let mut visited_forms = HashSet::new();
    let mut visited_resource_dicts = HashSet::new();
    let mut direct_image_count = 0usize;

    if let Ok((resource_dict, resource_ids)) = document.get_page_resources(page_id) {
        if let Some(resources) = resource_dict {
            collect_images_from_resource_dict(
                document,
                resources,
                &mut image_ids,
                &mut visited_forms,
                &mut visited_resource_dicts,
                &mut direct_image_count,
            );
        }

        for resource_id in resource_ids {
            if !visited_resource_dicts.insert(resource_id) {
                continue;
            }

            if let Ok(resources) = document.get_dictionary(resource_id) {
                collect_images_from_resource_dict(
                    document,
                    resources,
                    &mut image_ids,
                    &mut visited_forms,
                    &mut visited_resource_dicts,
                    &mut direct_image_count,
                );
            }
        }
    }

    (image_ids, direct_image_count)
}

fn collect_images_from_resource_dict(
    document: &Document,
    resources: &Dictionary,
    image_ids: &mut HashSet<ObjectId>,
    visited_forms: &mut HashSet<ObjectId>,
    visited_resource_dicts: &mut HashSet<ObjectId>,
    direct_image_count: &mut usize,
) {
    let Some(xobjects) = resources
        .get(b"XObject")
        .ok()
        .and_then(|object| resolve_dictionary_object(document, object))
    else {
        return;
    };

    for (_, xobject) in xobjects.iter() {
        collect_images_from_xobject(
            document,
            xobject,
            image_ids,
            visited_forms,
            visited_resource_dicts,
            direct_image_count,
        );
    }
}

fn collect_images_from_xobject(
    document: &Document,
    xobject: &Object,
    image_ids: &mut HashSet<ObjectId>,
    visited_forms: &mut HashSet<ObjectId>,
    visited_resource_dicts: &mut HashSet<ObjectId>,
    direct_image_count: &mut usize,
) {
    match xobject {
        Object::Reference(object_id) => {
            let Ok(object) = document.get_object(*object_id) else {
                return;
            };
            let Ok(stream) = object.as_stream() else {
                return;
            };

            if stream_has_subtype(stream, b"Image") {
                image_ids.insert(*object_id);
                return;
            }

            if stream_has_subtype(stream, b"Form") && visited_forms.insert(*object_id) {
                collect_images_from_form_stream(
                    document,
                    stream,
                    image_ids,
                    visited_forms,
                    visited_resource_dicts,
                    direct_image_count,
                );
            }
        }
        Object::Stream(stream) => {
            if stream_has_subtype(stream, b"Image") {
                *direct_image_count += 1;
                return;
            }

            if stream_has_subtype(stream, b"Form") {
                collect_images_from_form_stream(
                    document,
                    stream,
                    image_ids,
                    visited_forms,
                    visited_resource_dicts,
                    direct_image_count,
                );
            }
        }
        _ => {}
    }
}

fn collect_images_from_form_stream(
    document: &Document,
    stream: &Stream,
    image_ids: &mut HashSet<ObjectId>,
    visited_forms: &mut HashSet<ObjectId>,
    visited_resource_dicts: &mut HashSet<ObjectId>,
    direct_image_count: &mut usize,
) {
    let Some(resources) = stream
        .dict
        .get(b"Resources")
        .ok()
        .and_then(|object| resolve_dictionary_object(document, object))
    else {
        return;
    };

    collect_images_from_resource_dict(
        document,
        resources,
        image_ids,
        visited_forms,
        visited_resource_dicts,
        direct_image_count,
    );
}

fn resolve_dictionary_object<'a>(
    document: &'a Document,
    object: &'a Object,
) -> Option<&'a Dictionary> {
    match object {
        Object::Reference(object_id) => document.get_dictionary(*object_id).ok(),
        Object::Dictionary(dictionary) => Some(dictionary),
        _ => None,
    }
}

fn page_has_text_showing_operations(document: &Document, page_id: ObjectId) -> bool {
    document
        .get_and_decode_page_content(page_id)
        .ok()
        .is_some_and(|content| {
            content
                .operations
                .iter()
                .any(|operation| matches!(operation.operator.as_ref(), "Tj" | "TJ" | "'" | "\""))
        })
}

fn page_has_font_resources(document: &Document, page_id: ObjectId) -> bool {
    match document.get_page_fonts(page_id) {
        Ok(fonts) => !fonts.is_empty(),
        Err(_) => false,
    }
}

fn stream_has_subtype(stream: &Stream, expected: &[u8]) -> bool {
    matches!(stream.dict.get(b"Subtype"), Ok(Object::Name(name)) if name.as_slice() == expected)
}

fn average_per_page(total: f32, page_count: usize) -> f32 {
    if page_count == 0 {
        0.0
    } else {
        total / page_count as f32
    }
}

fn ratio(count: usize, total: usize) -> f32 {
    if total == 0 {
        0.0
    } else {
        count as f32 / total as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommend_max_image_size_px_defaults_without_edges() {
        assert_eq!(
            recommend_max_image_size_px(CompressionPreset::Balanced, &[]),
            CompressionPreset::Balanced.default_max_image_size_px()
        );
    }

    #[test]
    fn recommend_max_image_size_px_respects_preset_scaling() {
        let edges = [2000u32];
        assert_eq!(
            recommend_max_image_size_px(CompressionPreset::Conservative, &edges),
            2000
        );
        assert_eq!(
            recommend_max_image_size_px(CompressionPreset::Balanced, &edges),
            1700
        );
        assert_eq!(
            recommend_max_image_size_px(CompressionPreset::Maximum, &edges),
            1400
        );
    }

    #[test]
    fn recommend_max_image_size_px_clamps_outlier_edges() {
        // A 10 000 px edge is clamped down to the 3200 px ceiling before scaling.
        let edges = [10_000u32];
        let maximum = recommend_max_image_size_px(CompressionPreset::Maximum, &edges);
        assert!((800..=3200).contains(&maximum));

        let tiny = [64u32];
        let conservative = recommend_max_image_size_px(CompressionPreset::Conservative, &tiny);
        assert!(conservative >= 800);
    }

    #[test]
    fn analysis_page_sample_covers_small_documents_fully() {
        let page_map: BTreeMap<u32, ObjectId> = (1..=5).map(|i| (i, (i, 0))).collect();
        let sample = build_analysis_page_sample(&page_map);
        assert_eq!(sample.len(), 5);
    }

    #[test]
    fn analysis_page_sample_is_bounded_and_spread() {
        let page_map: BTreeMap<u32, ObjectId> = (1..=500).map(|i| (i, (i, 0))).collect();
        let sample = build_analysis_page_sample(&page_map);

        assert!(sample.len() <= MAX_ANALYSIS_SAMPLE_PAGES);
        assert!(!sample.is_empty());

        // Pages are returned in ascending order and span the document.
        let numbers: Vec<u32> = sample.iter().map(|(n, _)| *n).collect();
        assert!(numbers.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(*numbers.first().unwrap(), 1);
        assert_eq!(*numbers.last().unwrap(), 500);
    }

    #[test]
    fn savings_estimate_tracks_image_bytes_and_preset() {
        // Image-free document: near the floor.
        let text_only = estimate_savings_percent(CompressionPreset::Balanced, 0, 1_000_000);
        assert!((3.0..=10.0).contains(&text_only));

        // Image-heavy document scales with the preset's re-encode ratio.
        let maximum = estimate_savings_percent(CompressionPreset::Maximum, 900_000, 1_000_000);
        let balanced = estimate_savings_percent(CompressionPreset::Balanced, 900_000, 1_000_000);
        let conservative =
            estimate_savings_percent(CompressionPreset::Conservative, 900_000, 1_000_000);
        assert!(conservative < balanced && balanced < maximum);
        assert!((40.0..=70.0).contains(&maximum));

        // Zero-size input must not divide by zero.
        assert!(estimate_savings_percent(CompressionPreset::Maximum, 0, 0) >= 3.0);
    }
}
