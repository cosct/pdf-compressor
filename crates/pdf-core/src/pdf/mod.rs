//! PDF processing modules — analysis and compression engines.
//! PDF 处理模块 — 分析引擎和压缩引擎。

mod analyzer;
mod compressor;
mod encode;
mod resources;
mod search;
mod settings;
mod target_size;
mod workers;

#[cfg(test)]
mod tests;

pub use analyzer::analyze_pdf_with_progress;
pub use compressor::compress_pdf_with_progress;
pub use settings::{BilevelCodec, CompressionSettings, CompressionSettingsOverrides};
pub use target_size::compress_pdf_to_target_size;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use lopdf::{Document, Object, Stream};

use crate::error::AppError;

/// Cooperative cancellation check — shared by the analyzer, the compressor,
/// the image codec, and the worker pools.
/// 协作式取消检查 — 分析器、压缩器、图片编解码与线程池共用。
pub(crate) fn ensure_not_cancelled(
    cancel_flag: &Arc<AtomicBool>,
    task_id: &str,
) -> Result<(), AppError> {
    if cancel_flag.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled(task_id.to_string()));
    }

    Ok(())
}

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

/// Guard against encrypted documents, with a supported escape hatch.
///
/// lopdf transparently decrypts owner-password-only files with the empty user
/// password during `Document::load`, strips `/Encrypt` from the trailer, and
/// records the unlock in `Document::encryption_state`. Files that need a real
/// password — or use DRM handlers like EBX — fail authentication, leave the
/// object graph unparsed, and are rejected here: re-saving one without its
/// `/Encrypt` dictionary would emit a corrupt shell document (a real
/// data-loss hazard reported as "success").
///
/// Returns `true` when the input was encrypted and unlocked with the empty
/// user password; the caller reports it so users know the output is plain.
/// 拒绝需要真实密码的加密文档；空用户密码可解锁的（仅 owner 密码）放行并告知。
pub(crate) fn ensure_not_encrypted(document: &mut Document) -> Result<bool, AppError> {
    if document.trailer.has(b"Encrypt") {
        if document.get_pages().is_empty() {
            return Err(AppError::Encrypted);
        }
        // Defensive: an /Encrypt that survived a parsed load (lopdf normally
        // strips it after a successful empty-password decrypt).
        document.trailer.remove(b"Encrypt");
        document.encryption_state = None;
        return Ok(true);
    }

    // lopdf records the state of a successful empty-password decrypt; a plain
    // re-save must not carry it into an incremental-save restore path.
    if document.encryption_state.is_some() {
        document.encryption_state = None;
        return Ok(true);
    }

    Ok(false)
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
