use std::{io, path::PathBuf};

use image::ImageError;
use thiserror::Error;

/// Central application error type.
///
/// Keeping backend failures in one enum makes it easier to return readable
/// command errors to the Vue frontend without spreading formatting logic across
/// every service module.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("The selected file does not exist: {0}")]
    MissingInput(PathBuf),

    #[error("The selected file is not a PDF: {0}")]
    InvalidPdfPath(PathBuf),

    #[error("Image processing failed: {0}")]
    Image(#[from] ImageError),

    #[error("Filesystem operation failed: {0}")]
    Io(#[from] io::Error),

    #[error("Failed to build the output PDF: {0}")]
    PdfBuild(String),
}

impl From<AppError> for String {
    fn from(value: AppError) -> Self {
        value.to_string()
    }
}
