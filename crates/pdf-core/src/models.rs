//! Shared request/response structures serialized between the engine and its
//! clients (Tauri IPC, CLI output).
//! 引擎与客户端（Tauri IPC、CLI 输出）之间序列化传输的共享请求/响应结构。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

fn default_config_version() -> u32 {
    2
}

/// v1→v2 config migration (0.8.0's CMYK default flip). A v1 config's
/// `cmyk_conversion: Some(false)` was the 0.7.x **default state** persisted
/// verbatim — the 0.7 GUI/CLI wrote the checkbox default as an explicit
/// value, so `Some(false)` cannot be distinguished from "never touched"
/// (one cannot explicitly opt out of a feature that was already off).
/// The migration therefore drops v1 `Some(false)` back to unset, letting
/// the new default (on) apply; `Some(true)` was always a deliberate opt-in
/// and survives. v2 configs keep their values verbatim — a `Some(false)`
/// written by 0.8.0+ is a genuine opt-out and must never be migrated away.
/// Returns the stamped config version.
pub fn migrate_cmyk_default_flip(version: u32, cmyk_conversion: &mut Option<bool>) -> u32 {
    if version < 2 {
        *cmyk_conversion = cmyk_conversion.filter(|value| *value);
        2
    } else {
        version
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CompressionSettingsPayload {
    pub preset: Option<String>,
    pub image_quality: Option<u8>,
    /// Absolute maximum image edge in pixels. When neither this nor
    /// `max_image_size_percent` is set, the preset table's percent applies;
    /// when both are given, this absolute value wins.
    #[serde(default)]
    pub max_image_size_px: Option<u16>,
    /// Size cap as a percentage of the document's largest image edge (the
    /// 0.9.0 unified semantics; `max_image_size_px` stays the absolute
    /// override).
    #[serde(default)]
    pub max_image_size_percent: Option<u16>,
    pub optimize_images: Option<bool>,
    pub compress_streams: Option<bool>,
    pub strip_metadata: Option<bool>,
    /// Re-encode color images as grayscale (best for black-and-white scans).
    #[serde(default)]
    pub grayscale: Option<bool>,
    /// Output codec for near-bilevel scanned images: `"jpeg"` (default) or
    /// `"ccitt-g4"` (lossless ITU T.6, best for text scans).
    #[serde(default)]
    pub bilevel_codec: Option<String>,
    /// Shrink embedded Type0/CIDFontType2 TrueType fonts to the used glyphs.
    #[serde(default)]
    pub subset_fonts: Option<bool>,
    /// Convert CMYK images to RGB for re-encoding (default on since 0.8.0;
    /// builds without the `cmyk-cms` feature keep CMYK untouched instead of
    /// converting naively — grayscale requests still imply the collapse).
    #[serde(default)]
    pub cmyk_conversion: Option<bool>,
    pub output_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CompressPdfRequest {
    pub path: Option<String>,
    pub input_path: Option<String>,
    pub task_id: Option<String>,
    /// Open password for encrypted PDFs. `None`/absent for plain files; a
    /// wrong password fails with `error.wrongPassword`.
    #[serde(default)]
    pub password: Option<String>,
    pub settings: Option<CompressionSettingsPayload>,
    pub preset: Option<String>,
    pub image_quality: Option<u8>,
    pub max_image_size_px: Option<u16>,
    /// Percent-of-document-max-image-edge cap (0.9.0 unified semantics);
    /// `max_image_size_px` stays the absolute override.
    #[serde(default)]
    pub max_image_size_percent: Option<u16>,
    pub optimize_images: Option<bool>,
    pub compress_streams: Option<bool>,
    pub strip_metadata: Option<bool>,
    pub remove_metadata: Option<bool>,
    pub output_dir: Option<String>,
    /// When set, the engine searches quality/edge parameters until the output
    /// fits this byte budget (best effort otherwise).
    pub target_size_bytes: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CompressScannedPdfRequest {
    pub path: Option<String>,
    pub input_path: Option<String>,
    pub task_id: Option<String>,
    /// Open password for encrypted PDFs — same semantics as
    /// `CompressPdfRequest.password`.
    #[serde(default)]
    pub password: Option<String>,
    pub settings: Option<CompressionSettingsPayload>,
    pub preset: Option<String>,
    pub image_quality: Option<u8>,
    /// Maximum image edge in pixels — same semantics as `CompressPdfRequest`.
    /// (The historical `downsampleDpi`/`targetDpi` names were misnomers: the
    /// value was always consumed as a pixel bound, not a DPI.)
    #[serde(default, alias = "downsampleDpi", alias = "targetDpi")]
    pub max_image_size_px: Option<u16>,
    /// Percent-of-document-max-image-edge cap — same semantics as
    /// `CompressPdfRequest`.
    #[serde(default)]
    pub max_image_size_percent: Option<u16>,
    pub grayscale: Option<bool>,
    /// Output codec for near-bilevel scanned images: `"jpeg"` (default) or
    /// `"ccitt-g4"`.
    #[serde(default)]
    pub bilevel_codec: Option<String>,
    pub strip_metadata: Option<bool>,
    pub remove_metadata: Option<bool>,
    pub output_dir: Option<String>,
    /// When set, the engine searches quality/edge parameters until the output
    /// fits this byte budget (best effort otherwise).
    pub target_size_bytes: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PresetProfilePayload {
    pub image_quality: u8,
    pub max_image_size_percent: u8,
    // The rest is optional so older config files keep parsing.
    #[serde(default)]
    pub optimize_images: Option<bool>,
    #[serde(default)]
    pub compress_streams: Option<bool>,
    #[serde(default)]
    pub strip_metadata: Option<bool>,
    #[serde(default)]
    pub grayscale: Option<bool>,
    /// `"jpeg"` (default) or `"ccitt-g4"` for near-bilevel scans.
    #[serde(default)]
    pub bilevel_codec: Option<String>,
    #[serde(default)]
    pub subset_fonts: Option<bool>,
    /// CMYK→RGB conversion for re-encoding (default on since 0.8.0).
    #[serde(default)]
    pub cmyk_conversion: Option<bool>,
}

impl Default for PresetProfilePayload {
    fn default() -> Self {
        Self {
            image_quality: 72,
            max_image_size_percent: 80,
            optimize_images: None,
            compress_streams: None,
            strip_metadata: None,
            grayscale: None,
            bilevel_codec: None,
            subset_fonts: None,
            cmyk_conversion: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PresetUserConfigPayload {
    #[serde(default = "default_config_version")]
    pub version: u32,
    #[serde(default)]
    pub presets: BTreeMap<String, PresetProfilePayload>,
}

impl Default for PresetUserConfigPayload {
    fn default() -> Self {
        Self {
            version: default_config_version(),
            presets: BTreeMap::new(),
        }
    }
}

/// Persisted quick-mode (right-click) profile, edited from the desktop
/// settings page and consumed by `pdf-compressor-cli quick`. Every field is
/// optional — an explicit CLI flag wins first, then this profile, then the
/// engine's built-in defaults.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct QuickProfilePayload {
    #[serde(default = "default_config_version")]
    pub version: u32,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub image_quality: Option<u8>,
    /// Absolute pixel cap. Since 0.9.0 the panel stores the percent form;
    /// this field keeps reading profiles saved by earlier versions.
    #[serde(default)]
    pub max_image_size_px: Option<u16>,
    /// Percent-of-document-max-image-edge cap (0.9.0 unified semantics —
    /// quick mode resolves against each file's own images, like the GUI).
    #[serde(default)]
    pub max_image_size_percent: Option<u8>,
    #[serde(default)]
    pub optimize_images: Option<bool>,
    #[serde(default)]
    pub compress_streams: Option<bool>,
    #[serde(default)]
    pub strip_metadata: Option<bool>,
    #[serde(default)]
    pub grayscale: Option<bool>,
    /// `"jpeg"` (default) or `"ccitt-g4"` for near-bilevel scans.
    #[serde(default)]
    pub bilevel_codec: Option<String>,
    #[serde(default)]
    pub subset_fonts: Option<bool>,
    /// CMYK→RGB conversion for re-encoding (default on since 0.8.0).
    #[serde(default)]
    pub cmyk_conversion: Option<bool>,
    /// Byte budget for target-size mode; absent means plain compression.
    #[serde(default)]
    pub target_size_bytes: Option<u32>,
}

impl Default for QuickProfilePayload {
    fn default() -> Self {
        Self {
            version: default_config_version(),
            preset: None,
            image_quality: None,
            max_image_size_px: None,
            max_image_size_percent: None,
            optimize_images: None,
            compress_streams: None,
            strip_metadata: None,
            grayscale: None,
            bilevel_codec: None,
            subset_fonts: None,
            cmyk_conversion: None,
            target_size_bytes: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct BuildFeatures {
    /// Real CMYK→sRGB color management (`cmyk-cms` feature, vendored
    /// LittleCMS). Feature-off builds refuse the color conversion and keep
    /// CMYK images untouched (except grayscale/G4 collapse intents).
    pub cmyk_cms: bool,
    /// JPX (JPEG 2000) decoding (`jpx` feature, vendored OpenJPEG).
    pub jpx: bool,
    /// CCITT Group 4 bi-level encode/decode (`ccitt` feature).
    pub ccitt: bool,
    /// Embedded font subsetting (`subset-fonts` feature).
    pub subset_fonts: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct BackendNotice {
    pub code: String,
    pub level: String,
    pub values: BTreeMap<String, String>,
    pub fallback: String,
}

impl BackendNotice {
    pub fn new(
        code: impl Into<String>,
        level: impl Into<String>,
        fallback: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            level: level.into(),
            values: BTreeMap::new(),
            fallback: fallback.into(),
        }
    }

    pub fn with_value(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.values.insert(key.into(), value.into());
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProgressUpdate {
    pub phase: String,
    pub percent: f32,
    pub message: Option<BackendNotice>,
}

impl ProgressUpdate {
    pub fn new(phase: impl Into<String>, percent: f32) -> Self {
        Self {
            phase: phase.into(),
            percent,
            message: None,
        }
    }

    pub fn with_message(mut self, message: BackendNotice) -> Self {
        self.message = Some(message);
        self
    }
}

/// Wire types follow JSON/JS number semantics: byte sizes as `f64` (JSON
/// numbers are doubles; exact up to 2^53) and counts as `u32`, because the
/// TypeScript bindings forbid BigInt-style integers to avoid precision loss.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResponse {
    pub file_size_bytes: f64,
    pub page_count: u32,
    pub image_object_count: u32,
    pub document_kind: String,
    pub scanned_confidence: f32,
    pub image_coverage: f32,
    pub estimated_savings_percent: f32,
    pub recommended_preset: String,
    pub max_image_edge_px: u16,
    pub recommended_max_image_size_px: u16,
    pub recommended_image_quality: u8,
    pub is_likely_scanned: bool,
    pub notices: Vec<BackendNotice>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CompressionResponse {
    pub output_path: String,
    pub original_size_bytes: f64,
    pub compressed_size_bytes: f64,
    pub saved_bytes: f64,
    pub savings_percent: f32,
    pub elapsed_ms: u32,
    pub images_recompressed: u32,
    pub images_skipped: u32,
    pub images_deduplicated: u32,
    pub streams_compressed: u32,
    pub metadata_removed: bool,
    pub output_was_smaller: bool,
    pub notices: Vec<BackendNotice>,
}
