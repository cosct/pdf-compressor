use crate::{
    error::AppError,
    models::{AnalysisResponse, CompressionResponse, CompressionSettingsPayload},
    pdf,
};

#[tauri::command]
pub fn analyze_pdf(
    path: Option<String>,
    input_path: Option<String>,
) -> Result<AnalysisResponse, String> {
    let requested_path = input_path.or(path).ok_or_else(|| {
        AppError::PdfBuild("No input path was provided to analyze_pdf.".to_string()).to_string()
    })?;

    pdf::analyze_pdf(&requested_path).map_err(String::from)
}

#[tauri::command]
pub fn compress_pdf(
    path: Option<String>,
    input_path: Option<String>,
    settings: Option<CompressionSettingsPayload>,
    preset: Option<String>,
    image_quality: Option<u8>,
    max_image_size_px: Option<u16>,
    optimize_images: Option<bool>,
    compress_streams: Option<bool>,
    strip_metadata: Option<bool>,
    remove_metadata: Option<bool>,
) -> Result<CompressionResponse, String> {
    let requested_path = input_path.or(path).ok_or_else(|| {
        AppError::PdfBuild("No input path was provided to compress_pdf.".to_string()).to_string()
    })?;

    let merged_settings = pdf::CompressionSettings::from_sources(
        settings,
        preset,
        image_quality,
        max_image_size_px,
        optimize_images,
        compress_streams,
        strip_metadata.or(remove_metadata),
    );

    pdf::compress_pdf(&requested_path, merged_settings).map_err(String::from)
}

#[tauri::command]
pub fn compress_scanned_pdf(
    path: Option<String>,
    input_path: Option<String>,
    settings: Option<CompressionSettingsPayload>,
    preset: Option<String>,
    image_quality: Option<u8>,
    downsample_dpi: Option<u16>,
    target_dpi: Option<u16>,
    grayscale: Option<bool>,
    strip_metadata: Option<bool>,
    remove_metadata: Option<bool>,
) -> Result<CompressionResponse, String> {
    let requested_path = input_path.or(path).ok_or_else(|| {
        AppError::PdfBuild("No input path was provided to compress_scanned_pdf.".to_string())
            .to_string()
    })?;

    let merged_settings = pdf::CompressionSettings::from_sources(
        settings,
        preset,
        image_quality,
        downsample_dpi.or(target_dpi),
        Some(!matches!(grayscale, Some(true))),
        Some(true),
        strip_metadata.or(remove_metadata),
    );

    pdf::compress_pdf(&requested_path, merged_settings).map_err(String::from)
}
