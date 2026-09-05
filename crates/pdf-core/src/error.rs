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

    #[error(
        "The selected file is too large to process safely ({size_bytes} bytes; the limit is {limit_bytes} bytes)"
    )]
    InputTooLarge { size_bytes: u64, limit_bytes: u64 },

    #[error("The PDF is DRM-encrypted with an unsupported security handler; it cannot be processed")]
    Encrypted,

    #[error("The PDF requires an open password; supply one to process the file")]
    PasswordRequired,

    #[error("The supplied password did not unlock the PDF")]
    WrongPassword,

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
            AppError::InputTooLarge {
                size_bytes,
                limit_bytes,
            } => Self {
                code: "error.inputTooLarge".to_string(),
                values: BTreeMap::from([
                    ("size".to_string(), humanized_size(size_bytes)),
                    ("limit".to_string(), humanized_size(limit_bytes)),
                ]),
                fallback: format!(
                    "The selected file is {} and exceeds the {} processing limit.",
                    humanized_size(size_bytes),
                    humanized_size(limit_bytes)
                ),
            },
            AppError::Encrypted => Self {
                code: "error.encryptedPdf".to_string(),
                values: BTreeMap::new(),
                fallback: "The PDF is DRM-encrypted with an unsupported security handler; \
                           it cannot be processed"
                    .to_string(),
            },
            AppError::PasswordRequired => Self {
                code: "error.passwordRequired".to_string(),
                values: BTreeMap::new(),
                fallback: "The PDF requires an open password; supply one to process the file."
                    .to_string(),
            },
            AppError::WrongPassword => Self {
                code: "error.wrongPassword".to_string(),
                values: BTreeMap::new(),
                fallback: "The supplied password did not unlock the PDF.".to_string(),
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

/// Human-friendly byte size for error messages: one decimal below 10, none
/// above (`2.1 GB`, `768 MB`, `512 KB`, `42 B`).
pub(crate) fn humanized_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanized_sizes_use_familiar_units() {
        assert_eq!(humanized_size(42), "42 B");
        assert_eq!(humanized_size(512 * 1024), "512 KB");
        assert_eq!(humanized_size(3 * 1024 * 1024 * 1024 / 2), "1.5 GB");
        assert_eq!(
            humanized_size(2 * 1024_u64.pow(3)),
            "2.0 GB",
            "the 2 GiB limit reads naturally"
        );
    }

    #[test]
    fn oversized_input_payload_carries_humanized_values() {
        let payload = AppErrorPayload::from(AppError::InputTooLarge {
            size_bytes: 2_700_000_000,
            limit_bytes: 2 * 1024 * 1024 * 1024,
        });
        assert_eq!(payload.code, "error.inputTooLarge");
        assert_eq!(payload.values.get("limit").map(String::as_str), Some("2.0 GB"));
        assert!(payload.fallback.contains("2.0 GB"));
    }
}
