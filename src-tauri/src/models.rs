use serde::{Deserialize, Serialize};

/// Raw settings payload received from the frontend.
///
/// The Vue layer already constrains the UI, but the backend still validates and
/// normalizes every field so commands remain safe if they are invoked manually.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressionSettingsPayload {
    pub preset: Option<String>,
    pub image_quality: Option<u8>,
    pub max_image_size_px: Option<u16>,
    pub optimize_images: Option<bool>,
    pub compress_streams: Option<bool>,
    pub strip_metadata: Option<bool>,
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
    pub is_likely_scanned: bool,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
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
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
}
