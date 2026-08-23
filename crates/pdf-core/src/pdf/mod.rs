//! PDF processing modules — analysis and compression engines.
//! PDF 处理模块 — 分析引擎和压缩引擎。

mod analyzer;
mod compressor;
mod settings;
mod target_size;

#[cfg(test)]
mod tests;

pub use analyzer::analyze_pdf_with_progress;
pub use compressor::compress_pdf_with_progress;
pub use settings::{CompressionSettings, CompressionSettingsOverrides};
pub use target_size::compress_pdf_to_target_size;

use lopdf::{Object, Stream};

use crate::error::AppError;

/// Validate a user-supplied path: must exist and have a `.pdf` extension.
/// Shared by the analyzer and compressor entry points.
pub(crate) fn validate_input_path(path: &str) -> Result<std::path::PathBuf, AppError> {
    let candidate = std::path::PathBuf::from(path);

    if !candidate.exists() {
        return Err(AppError::MissingInput(candidate));
    }

    let is_pdf = candidate
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"));

    if !is_pdf {
        return Err(AppError::InvalidPdfPath(candidate));
    }

    Ok(candidate)
}

/// Read an optional integer value from a PDF stream dictionary.
/// Handles both `Integer` and `Real` (truncated to `i64`) object types.
pub(crate) fn optional_integer(stream: &Stream, key: &[u8]) -> Option<i64> {
    match stream.dict.get(key) {
        Ok(Object::Integer(v)) => Some(*v),
        Ok(Object::Real(v)) => Some(*v as i64),
        _ => None,
    }
}
