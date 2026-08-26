//! Centralized backend error types with i18n-compatible error codes.
//! 集中式后端错误类型，包含 i18n 兼容的错误码。
//!
//! `AppError` is the internal error enum; `AppErrorPayload` is the serializable
//! representation sent to clients with a code, values map, and fallback message.
//! `AppError` 是内部错误枚举；`AppErrorPayload` 是发送给客户端的可序列化表示，
//! 包含错误码、值映射和回退消息。

use std::{collections::BTreeMap, io, path::PathBuf};

use image::ImageError;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("The selected file does not exist: {0}")]
    MissingInput(PathBuf),

    #[error("The selected file is not a PDF: {0}")]
    InvalidPdfPath(PathBuf),

    #[error("The PDF is password-protected or DRM-encrypted; encrypted documents are not supported")]
    Encrypted,

    #[error("Image processing failed: {0}")]
    Image(#[from] ImageError),

    #[error("Filesystem operation failed: {0}")]
    Io(#[from] io::Error),

    #[error("Compression cancelled for task: {0}")]
    Cancelled(String),

    #[error("Configuration operation failed: {0}")]
    Config(String),

    #[error("Failed to open the path with the system handler: {0}")]
    Opener(String),

    #[error("Failed to build the output PDF: {0}")]
    PdfBuild(String),
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AppErrorPayload {
    pub code: String,
    pub values: BTreeMap<String, String>,
    pub fallback: String,
}

impl From<AppError> for AppErrorPayload {
    fn from(value: AppError) -> Self {
        match value {
            AppError::MissingInput(path) => Self {
                code: "error.missingInput".to_string(),
                values: BTreeMap::from([("path".to_string(), path.to_string_lossy().to_string())]),
                fallback: format!(
                    "The selected file does not exist: {}",
                    path.to_string_lossy()
                ),
            },
            AppError::InvalidPdfPath(path) => Self {
                code: "error.invalidPdfPath".to_string(),
                values: BTreeMap::from([("path".to_string(), path.to_string_lossy().to_string())]),
                fallback: format!("The selected file is not a PDF: {}", path.to_string_lossy()),
            },
            AppError::Encrypted => Self {
                code: "error.encryptedPdf".to_string(),
                values: BTreeMap::new(),
                fallback: "The PDF is password-protected or DRM-encrypted; encrypted \
                           documents are not supported"
                    .to_string(),
            },
            AppError::Image(error) => Self {
                code: "error.image".to_string(),
                values: BTreeMap::from([("detail".to_string(), error.to_string())]),
                fallback: format!("Image processing failed: {error}"),
            },
            AppError::Io(error) => Self {
                code: "error.io".to_string(),
                values: BTreeMap::from([("detail".to_string(), error.to_string())]),
                fallback: format!("Filesystem operation failed: {error}"),
            },
            AppError::Cancelled(task_id) => Self {
                code: "error.cancelled".to_string(),
                values: BTreeMap::from([("taskId".to_string(), task_id.clone())]),
                fallback: format!("Compression cancelled for task: {task_id}"),
            },
            AppError::Config(detail) => Self {
                code: "error.config".to_string(),
                values: BTreeMap::from([("detail".to_string(), detail.clone())]),
                fallback: format!("Configuration operation failed: {detail}"),
            },
            AppError::Opener(detail) => Self {
                code: "error.opener".to_string(),
                values: BTreeMap::from([("detail".to_string(), detail.clone())]),
                fallback: format!("Failed to open the path with the system handler: {detail}"),
            },
            AppError::PdfBuild(detail) => Self {
                code: "error.pdfBuild".to_string(),
                values: BTreeMap::from([("detail".to_string(), detail.clone())]),
                fallback: format!("Failed to build the output PDF: {detail}"),
            },
        }
    }
}
