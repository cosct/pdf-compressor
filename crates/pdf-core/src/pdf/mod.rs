//! PDF processing modules — analysis and compression engines.
//! PDF 处理模块 — 分析引擎和压缩引擎。

mod analyzer;
#[cfg(feature = "subset-fonts")]
mod cff;
mod cmyk;
mod colorspace;
mod compressor;
mod encode;
#[cfg(feature = "subset-fonts")]
mod fonts;
mod jbig2;
#[cfg(feature = "jpx")]
mod jpx;
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

/// Load a document, supplying the open password when one was given. A failed
/// load of a user-password file does not error here — lopdf returns a
/// structural document with an unparsed object graph, which
/// [`ensure_not_encrypted`] then classifies (missing vs wrong password vs
/// unsupported DRM handler).
pub(crate) fn load_document(
    input_path: &std::path::Path,
    password: Option<&str>,
) -> Result<Document, AppError> {
    let password = password.filter(|value| !value.is_empty());
    let loaded = match &password {
        Some(password) => Document::load_with_password(input_path, password),
        None => Document::load(input_path),
    };
    // lopdf fails the load outright on a wrong password (unlike the no-password
    // path, which returns an unparsed shell document).
    if matches!(loaded, Err(lopdf::Error::InvalidPassword)) {
        return Err(AppError::WrongPassword);
    }
    loaded.map_err(|e| AppError::PdfBuild(format!("Failed to load PDF: {e}")))
}

/// Guard against encrypted documents, with supported escape hatches.
///
/// lopdf transparently decrypts owner-password-only files with the empty user
/// password during `Document::load`, strips `/Encrypt` from the trailer, and
/// records the unlock in `Document::encryption_state`. Files that need a real
/// password — or use DRM handlers like EBX — fail authentication, leave the
/// object graph unparsed, and are classified here: with no password supplied
/// that is `PasswordRequired`; after a failed password attempt it is
/// `WrongPassword` (or `Encrypted` for handlers no password can satisfy).
/// Re-saving an unauthenticated document without its `/Encrypt` dictionary
/// would emit a corrupt shell (a real data-loss hazard reported as "success").
///
/// Returns `true` when the input was encrypted and unlocked with the empty user
/// password; the caller reports it so users know the output is plain.
/// 拒绝无法解锁的加密文档；空用户密码可解锁的（仅 owner 密码）放行并告知，
/// 提供了密码的按密码加载，失败则区分缺密码/密码错误。
pub(crate) fn ensure_not_encrypted(
    document: &mut Document,
    password_attempted: bool,
) -> Result<bool, AppError> {
    if document.trailer.has(b"Encrypt") {
        if document.get_pages().is_empty() {
            return Err(if password_attempted {
                // A password was tried and the object graph still did not
                // parse: either the password is wrong or the handler is
                // unsupported DRM. Report the actionable case; DRM files are
                // rare enough that a wrong-password message is still honest.
                AppError::WrongPassword
            } else {
                AppError::PasswordRequired
            });
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
