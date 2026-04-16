//! Shared request/response structures serialized between Rust and Vue via Tauri IPC.
//! Rust 与 Vue 之间通过 Tauri IPC 序列化传输的共享请求/响应结构。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

fn default_preset_config_version() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressionSettingsPayload {
    pub preset: Option<String>,
    pub image_quality: Option<u8>,
    pub max_image_size_px: Option<u16>,
    pub optimize_images: Option<bool>,
    pub compress_streams: Option<bool>,
    pub strip_metadata: Option<bool>,
    pub output_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressPdfRequest {
    pub path: Option<String>,
    pub input_path: Option<String>,
    pub task_id: Option<String>,
    pub settings: Option<CompressionSettingsPayload>,
    pub preset: Option<String>,
    pub image_quality: Option<u8>,
    pub max_image_size_px: Option<u16>,
    pub optimize_images: Option<bool>,
    pub compress_streams: Option<bool>,
    pub strip_metadata: Option<bool>,
    pub remove_metadata: Option<bool>,
    pub output_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressScannedPdfRequest {
    pub path: Option<String>,
    pub input_path: Option<String>,
    pub task_id: Option<String>,
    pub settings: Option<CompressionSettingsPayload>,
    pub preset: Option<String>,
    pub image_quality: Option<u8>,
    pub downsample_dpi: Option<u16>,
    pub target_dpi: Option<u16>,
    pub grayscale: Option<bool>,
    pub strip_metadata: Option<bool>,
    pub remove_metadata: Option<bool>,
    pub output_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetProfilePayload {
    pub image_quality: u8,
    pub max_image_size_percent: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetUserConfigPayload {
    #[serde(default = "default_preset_config_version")]
    pub version: u32,
    #[serde(default)]
    pub presets: BTreeMap<String, PresetProfilePayload>,
}

impl Default for PresetUserConfigPayload {
    fn default() -> Self {
        Self {
            version: default_preset_config_version(),
            presets: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResponse {
    pub file_size_bytes: u64,
    pub page_count: usize,
    pub image_object_count: usize,
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
#[serde(rename_all = "camelCase")]
pub struct CompressionResponse {
    pub output_path: String,
    pub original_size_bytes: u64,
    pub compressed_size_bytes: u64,
    pub saved_bytes: u64,
    pub savings_percent: f32,
    pub elapsed_ms: u128,
    pub images_recompressed: usize,
    pub images_skipped: usize,
    pub streams_compressed: usize,
    pub metadata_removed: bool,
    pub output_was_smaller: bool,
    pub notices: Vec<BackendNotice>,
}
