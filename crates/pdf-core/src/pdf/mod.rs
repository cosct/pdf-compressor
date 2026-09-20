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
pub use compressor::{
    compress_pdf_bytes_with_progress, compress_pdf_with_progress, BytesCompressionOutcome,
    MAX_INPUT_BYTES,
};
pub use settings::{BilevelCodec, CompressionSettings, CompressionSettingsOverrides};
pub use target_size::{compress_pdf_bytes_to_target_size, compress_pdf_to_target_size};

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use lopdf::{Document, Object, ObjectId, Stream};

use crate::error::AppError;

/// Ceiling for any single stream's decompressed output while the document is
/// loading (object and xref streams decode eagerly, before any engine code
/// runs). Bounds the classic "tiny flate stream, huge expansion" bomb at the
/// loader; per-consumer sites apply tighter `*_with_limit` caps on top.
pub(crate) const MAX_LOAD_DECOMPRESSED_BYTES: usize = 512 * 1024 * 1024;

/// Ceiling on total pixels a single image decode may allocate before it runs
/// (100 megapixels ≈ 100 MB luma / 300 MB RGB). Per-edge caps alone bound the
/// axes, not the product — 65535 × 65535 would otherwise request a 4.29 GB
/// plane from a stream a few bytes long.
pub(crate) const MAX_DECODE_PIXELS: u64 = 100_000_000;

/// Bomb cap for one page's (or form's) concatenated content streams. Real
/// page content is KB-to-low-MB scale; anything inflating past this is not
/// legitimate drawing instructions.
pub(crate) const MAX_CONTENT_STREAM_BYTES: usize = 64 << 20;

/// Bomb cap for one `/JBIG2Globals` segment stream (shared symbol
/// dictionaries are KB-to-MB scale in real scans).
pub(crate) const MAX_JBIG2_GLOBALS_BYTES: usize = 64 << 20;

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
    let loaded = Document::load_with_options(input_path, load_options(password));
    classify_document_load(loaded)
}

/// In-memory twin of [`load_document`] for the bytes pipeline (CLI
/// stdin/stdout mode): same password semantics, same error classification.
pub(crate) fn load_document_mem(
    bytes: &[u8],
    password: Option<&str>,
) -> Result<Document, AppError> {
    let password = password.filter(|value| !value.is_empty());
    let loaded = Document::load_mem_with_options(bytes, load_options(password));
    classify_document_load(loaded)
}

/// Loader options shared by both entry points: open password when given, and
/// the decompression-bomb ceiling on eagerly decoded object/xref streams.
fn load_options(password: Option<&str>) -> lopdf::LoadOptions {
    lopdf::LoadOptions {
        password: password.map(str::to_string),
        max_decompressed_size: Some(MAX_LOAD_DECOMPRESSED_BYTES),
        ..Default::default()
    }
}

/// Whether declared image dimensions pass the shared decode budget: both
/// edges in `1..=65535` (the JPEG re-encoder's format limit) and the pixel
/// product under [`MAX_DECODE_PIXELS`]. Every image decoder gates on this
/// before allocating; `None` means the shape must not be decoded.
pub(crate) fn image_dims_within_budget(width: i64, height: i64) -> Option<(u32, u32)> {
    if !(1..=65_535).contains(&width) || !(1..=65_535).contains(&height) {
        return None;
    }
    let pixels = (width as u64) * (height as u64);
    if pixels > MAX_DECODE_PIXELS {
        return None;
    }
    Some((width as u32, height as u32))
}

/// Bomb-safe twin of `Document::get_and_decode_page_content`: the page's
/// concatenated content streams decode against `limit` before the operation
/// parser runs. `None` (missing stream, over the limit, unparsable) means
/// callers skip that page, matching the unbounded version's error handling.
pub(crate) fn decoded_page_content_with_limit(
    document: &Document,
    page_id: ObjectId,
    limit: usize,
) -> Option<lopdf::content::Content> {
    let bytes = document.get_page_content_with_limit(page_id, limit).ok()?;
    lopdf::content::Content::decode(&bytes).ok()
}

/// Map a lopdf load result onto the engine's error taxonomy. lopdf fails the
/// load outright on a wrong password (unlike the no-password path, which
/// returns an unparsed shell document).
fn classify_document_load(loaded: Result<Document, lopdf::Error>) -> Result<Document, AppError> {
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
            // A password was tried and the object graph still did not parse.
            // Distinguish what a password can and cannot fix: a non-standard
            // security handler (EBX and friends) rejects every password by
            // design, and reporting "wrong password" there sends the user
            // into a retry loop no password can satisfy.
            let drm_handler = document
                .get_encrypted()
                .ok()
                .and_then(|encrypt| encrypt.get(b"Filter").ok())
                .is_some_and(|filter| !matches!(filter, Object::Name(name) if name.as_slice() == b"Standard"));
            return Err(if drm_handler {
                AppError::Encrypted
            } else if password_attempted {
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

#[cfg(test)]
mod optional_integer_tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn optional_integer_reads_integer_and_truncated_real() {
        // Producer PDFs occasionally store integral values as Real (e.g.
        // `612.0`); the width/height readers rely on the truncating arm.
        let integer = Stream::new(dictionary! { "Width" => 612 }, Vec::new());
        assert_eq!(optional_integer(&integer, b"Width"), Some(612));

        let real = Stream::new(dictionary! { "Width" => Object::Real(612.7) }, Vec::new());
        assert_eq!(
            optional_integer(&real, b"Width"),
            Some(612),
            "Real values truncate toward zero"
        );

        let textual = Stream::new(dictionary! { "Width" => "612" }, Vec::new());
        assert_eq!(optional_integer(&textual, b"Width"), None);
        let missing = Stream::new(dictionary! {}, Vec::new());
        assert_eq!(optional_integer(&missing, b"Width"), None);
    }
}
