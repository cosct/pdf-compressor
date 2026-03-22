use std::{collections::HashSet, fs, path::PathBuf};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::{error::AppError, models::AnalysisResponse, pdf::settings::CompressionPreset};

const TEXT_NATIVE_KIND: &str = "text-native";
const MIXED_KIND: &str = "mixed";
const SCAN_HEAVY_KIND: &str = "scan-heavy";

#[derive(Debug, Default)]
struct AnalysisSignals {
    // Count non-whitespace characters from directly extractable text as the
    // strongest signal that the PDF still contains searchable text.
    extracted_text_characters: usize,
    // Track multiple structural text signals because the pure-Rust analysis
    // path cannot render pages and must cross-check the document structure.
    pages_with_extractable_text: usize,
    pages_with_text_showing_ops: usize,
    pages_with_font_resources: usize,
    pages_with_images: usize,
    total_page_image_references: usize,
    // Extraction failures lower confidence, but they are not treated as proof
    // that the document is scan-heavy.
    text_extraction_failures: usize,
}

/// Performs a lightweight preflight pass so the UI can estimate how much of the
/// document can be optimized without flattening text and vector content.
pub fn analyze_pdf(path: &str) -> Result<AnalysisResponse, AppError> {
    let input_path = validate_input_path(path)?;
    let file_size_bytes = fs::metadata(&input_path)?.len();

    let document = Document::load(&input_path)
        .map_err(|error| AppError::PdfBuild(format!("Failed to inspect PDF structure: {error}")))?;
    let page_map = document.get_pages();
    let page_count = page_map.len();
    let image_object_count = count_image_stream_objects(&document);
    let signals = collect_analysis_signals(&document, &page_map);

    let average_bytes_per_page = average_per_page(file_size_bytes as f32, page_count);
    let average_text_per_page =
        average_per_page(signals.extracted_text_characters as f32, page_count);
    let average_images_per_page = if signals.total_page_image_references > 0 {
        average_per_page(signals.total_page_image_references as f32, page_count)
    } else {
        average_per_page(image_object_count as f32, page_count)
    };
    let text_page_ratio = ratio(signals.pages_with_extractable_text, page_count);
    let structural_text_ratio = ratio(
        signals
            .pages_with_extractable_text
            .max(signals.pages_with_text_showing_ops)
            .max(signals.pages_with_font_resources),
        page_count,
    );
    let image_page_ratio = ratio(signals.pages_with_images, page_count);
    let extraction_uncertainty_ratio = ratio(signals.text_extraction_failures, page_count);

    // Stay conservative here: if the structure is ambiguous, prefer `mixed`
    // over overstating a still-editable document as `scan-heavy`.
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

    let estimated_savings_percent = match recommended_preset {
        CompressionPreset::Maximum => 28.0,
        CompressionPreset::Balanced => 18.0,
        CompressionPreset::Conservative => 10.0,
    };

    let mut warnings = Vec::new();
    let mut notes = vec![
        "This analysis uses pure-Rust PDF structure inspection with lopdf, so text density and image coverage are estimated from extractable text and page resources rather than a renderer.".to_string(),
        "The compression workflow still preserves text and vector instructions whenever possible and focuses first on embedded image streams, metadata, and compressible PDF streams.".to_string(),
    ];

    if signals.text_extraction_failures > 0 {
        warnings.push(
            "Some pages expose text or font structure that could not be fully decoded, so this recommendation stays intentionally conservative."
                .to_string(),
        );
    }

    if document_kind == TEXT_NATIVE_KIND {
        warnings.push(
            "This file looks mostly text-native, so the optimizer will likely preserve structure but may only save a modest amount of space."
                .to_string(),
        );
    }

    if page_count > 150 {
        warnings.push(
            "This PDF has many pages, so compression may take noticeably longer than small documents.".to_string(),
        );
    }

    if image_object_count == 0 {
        warnings.push(
            "No embedded image objects were detected, so most savings will depend on stream compression and metadata cleanup.".to_string(),
        );
    }

    if document_kind == MIXED_KIND && text_page_ratio > 0.25 && image_page_ratio > 0.25 {
        notes.push(
            "This PDF mixes readable text structure with image-heavy pages, so the suggested preset favors a safer first pass over aggressive rewriting."
                .to_string(),
        );
    }

    if file_size_bytes < 1_000_000 {
        notes.push("Small PDFs often have less room to shrink dramatically.".to_string());
    }

    Ok(AnalysisResponse {
        file_size_bytes,
        page_count: page_count.into(),
        image_object_count,
        document_kind: document_kind.to_string(),
        scanned_confidence,
        image_coverage,
        estimated_savings_percent,
        recommended_preset: recommended_preset.as_label().to_string(),
        is_likely_scanned,
        warnings,
        notes,
    })
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

fn collect_analysis_signals(
    document: &Document,
    page_map: &std::collections::BTreeMap<u32, ObjectId>,
) -> AnalysisSignals {
    let mut signals = AnalysisSignals::default();

    for (&page_number, &page_id) in page_map {
        // Collect text, image, font, and operator signals together so a single
        // parser limitation does not collapse the whole page classification.
        let (page_text_characters, had_text_extraction_error) =
            extract_page_text_characters(document, page_number);
        let (page_image_references, direct_image_references) =
            collect_page_image_references(document, page_id);
        let has_images = !page_image_references.is_empty() || direct_image_references > 0;
        let has_text_showing_ops = page_has_text_showing_operations(document, page_id);
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
        // 先看页面直接资源，再递归追踪 Form XObject 中继续嵌套的资源字典。
        // 这样可以尽量覆盖“页面引用了表单，表单里再引用图片”的常见结构。
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
            // 这里只判断是否出现典型文本绘制操作，不尝试重建最终排版。
            // 它的作用是给“提取不到文本但页面看起来像有文本结构”的 PDF 一个兜底信号。
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

fn count_image_stream_objects(document: &Document) -> usize {
    document
        .objects
        .values()
        .filter(|object| {
            matches!(object, Object::Stream(stream) if stream_has_subtype(stream, b"Image"))
        })
        .count()
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
