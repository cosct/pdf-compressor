//! PDF processing modules — analysis and compression engines.
//! PDF 处理模块 — 分析引擎和压缩引擎。

mod analyzer;
mod compressor;
mod settings;

pub use analyzer::analyze_pdf_with_progress;
pub use compressor::compress_pdf_with_progress;
pub use settings::{CompressionSettings, CompressionSettingsOverrides};

use lopdf::{Object, Stream};

/// Read an optional integer value from a PDF stream dictionary.
/// Handles both `Integer` and `Real` (truncated to `i64`) object types.
pub(crate) fn optional_integer(stream: &Stream, key: &[u8]) -> Option<i64> {
    match stream.dict.get(key) {
        Ok(Object::Integer(v)) => Some(*v),
        Ok(Object::Real(v)) => Some(*v as i64),
        _ => None,
    }
}
