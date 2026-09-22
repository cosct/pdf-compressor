//! DTO mirror of the engine's wire models — the mobile subset.
//! 引擎线上传输模型的移动端子集镜像。
//!
//! Field-for-field these mirror `pdf-core/src/models.rs` (camelCase there,
//! snake_case here because UniFFI lowercases into Kotlin): sizes widen to
//! `u64` (exact, unlike the desktop's JSON-safe `f64`), `output_path`/
//! `output_dir` drop away (SAF owns output locations), and every notice
//! crosses verbatim so the Kotlin locales can interpolate the same value
//! keys the desktop does. The parity pins in `tests/parity.rs` freeze this
//! field set against the engine's.

use std::collections::HashMap;

use pdf_core::models::{AnalysisResponse, BackendNotice, CompressionSettingsPayload};
use pdf_core::{BuildFeatures, BytesCompressionOutcome};

/// Backend notice: code + level + i18n values + English fallback — identical
/// contract to the desktop's `BackendNotice`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiNotice {
    pub code: String,
    pub level: String,
    pub values: HashMap<String, String>,
    pub fallback: String,
}

impl From<BackendNotice> for FfiNotice {
    fn from(notice: BackendNotice) -> Self {
        Self {
            code: notice.code,
            level: notice.level,
            values: notice.values.into_iter().collect(),
            fallback: notice.fallback,
        }
    }
}

/// Mobile mirror of `CompressionSettingsPayload` — every knob the engine
/// accepts that makes sense without a filesystem. All-`Option` with the
/// same merge semantics: `None` fields fall through to the preset table /
/// engine defaults via `CompressionSettings::from_sources`.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct FfiSettings {
    pub preset: Option<String>,
    pub image_quality: Option<u8>,
    /// Absolute maximum image edge in pixels; when neither this nor
    /// `max_image_size_percent` is set, the preset table's percent applies.
    pub max_image_size_px: Option<u16>,
    /// Size cap as a percentage of the document's largest image edge.
    pub max_image_size_percent: Option<u16>,
    pub optimize_images: Option<bool>,
    pub compress_streams: Option<bool>,
    pub strip_metadata: Option<bool>,
    /// Re-encode color images as grayscale (best for black-and-white scans).
    pub grayscale: Option<bool>,
    /// Output codec for near-bilevel scans: `"ccitt-g4"` (default) or `"jpeg"`.
    pub bilevel_codec: Option<String>,
    /// Shrink embedded Type0/CIDFontType2 fonts to the used glyphs.
    pub subset_fonts: Option<bool>,
    /// Convert CMYK images to RGB for re-encoding (default on; feature-off
    /// builds keep CMYK untouched — see `FfiBuildFeatures.cmyk_cms`).
    pub cmyk_conversion: Option<bool>,
}

impl FfiSettings {
    /// Into the engine's payload: the exact same struct the desktop IPC
    /// deserializes, with `output_dir` pinned to `None` (SAF owns output).
    pub fn to_payload(self) -> CompressionSettingsPayload {
        CompressionSettingsPayload {
            preset: self.preset,
            image_quality: self.image_quality,
            max_image_size_px: self.max_image_size_px,
            max_image_size_percent: self.max_image_size_percent,
            optimize_images: self.optimize_images,
            compress_streams: self.compress_streams,
            strip_metadata: self.strip_metadata,
            grayscale: self.grayscale,
            bilevel_codec: self.bilevel_codec,
            subset_fonts: self.subset_fonts,
            cmyk_conversion: self.cmyk_conversion,
            output_dir: None,
        }
    }
}

/// Mirror of `AnalysisResponse`.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAnalysis {
    pub file_size_bytes: u64,
    pub page_count: u32,
    pub image_object_count: u32,
    /// `text-native` / `mixed` / `scan-heavy` — kept open (not an enum) so
    /// an engine-side addition is not an FFI break; Kotlin maps knowns.
    pub document_kind: String,
    pub scanned_confidence: f32,
    pub image_coverage: f32,
    pub estimated_savings_percent: f32,
    pub recommended_preset: String,
    pub max_image_edge_px: u16,
    pub recommended_max_image_size_px: u16,
    pub recommended_image_quality: u8,
    pub is_likely_scanned: bool,
    pub notices: Vec<FfiNotice>,
}

impl From<AnalysisResponse> for FfiAnalysis {
    fn from(response: AnalysisResponse) -> Self {
        Self {
            file_size_bytes: response.file_size_bytes as u64,
            page_count: response.page_count,
            image_object_count: response.image_object_count,
            document_kind: response.document_kind,
            scanned_confidence: response.scanned_confidence,
            image_coverage: response.image_coverage,
            estimated_savings_percent: response.estimated_savings_percent,
            recommended_preset: response.recommended_preset,
            max_image_edge_px: response.max_image_edge_px,
            recommended_max_image_size_px: response.recommended_max_image_size_px,
            recommended_image_quality: response.recommended_image_quality,
            is_likely_scanned: response.is_likely_scanned,
            notices: response.notices.into_iter().map(FfiNotice::from).collect(),
        }
    }
}

/// Mirror of `CompressionResponse` plus the output bytes: the pipe's
/// passthrough rule ("hand back the original when compression could not
/// beat it") means `bytes` is always a valid PDF — `output_was_smaller`
/// says which side of the passthrough it is.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiCompressResult {
    pub bytes: Vec<u8>,
    pub original_size_bytes: u64,
    pub compressed_size_bytes: u64,
    pub saved_bytes: u64,
    pub savings_percent: f32,
    pub elapsed_ms: u32,
    pub images_recompressed: u32,
    pub images_skipped: u32,
    pub images_deduplicated: u32,
    pub streams_compressed: u32,
    pub metadata_removed: bool,
    pub output_was_smaller: bool,
    pub notices: Vec<FfiNotice>,
}

impl From<BytesCompressionOutcome> for FfiCompressResult {
    fn from(outcome: BytesCompressionOutcome) -> Self {
        let response = outcome.response;
        Self {
            bytes: outcome.bytes,
            original_size_bytes: response.original_size_bytes as u64,
            compressed_size_bytes: response.compressed_size_bytes as u64,
            saved_bytes: response.saved_bytes as u64,
            savings_percent: response.savings_percent,
            elapsed_ms: response.elapsed_ms,
            images_recompressed: response.images_recompressed,
            images_skipped: response.images_skipped,
            images_deduplicated: response.images_deduplicated,
            streams_compressed: response.streams_compressed,
            metadata_removed: response.metadata_removed,
            output_was_smaller: response.output_was_smaller,
            notices: response.notices.into_iter().map(FfiNotice::from).collect(),
        }
    }
}

/// One row of the preset table, read straight from the engine's
/// `CompressionPreset` — the single source of truth (the same values the
/// desktop panel shows). `max_image_size_px` is the percent resolved
/// against the default reference edge, matching the analyzer's fallback.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiPresetProfile {
    pub name: String,
    pub image_quality: u8,
    pub max_image_size_percent: u16,
    pub max_image_size_px: u16,
    pub strip_metadata: bool,
    pub subset_fonts: bool,
}

/// Mirror of the engine's `BuildFeatures` — what this build's engine can
/// actually do, surfaced so the UI can honestly reflect it (e.g. the CMYK
/// toggle on a `cmyk-cms`-less build does nothing).
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiBuildFeatures {
    pub cmyk_cms: bool,
    pub jpx: bool,
    pub ccitt: bool,
    pub subset_fonts: bool,
}

impl From<BuildFeatures> for FfiBuildFeatures {
    fn from(features: BuildFeatures) -> Self {
        Self {
            cmyk_cms: features.cmyk_cms,
            jpx: features.jpx,
            ccitt: features.ccitt,
            subset_fonts: features.subset_fonts,
        }
    }
}
