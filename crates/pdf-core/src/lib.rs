//! Pure PDF analysis & compression engine — no Tauri, no UI.
//! 纯 Rust PDF 分析与压缩引擎 — 不依赖 Tauri / UI。
//!
//! This crate is the reusable core: it owns the analyzer, compressor,
//! settings normalization, wire models, and error types. The Tauri app, the
//! criterion bench, the `pdf-compressor-cli` binary, and cargo-fuzz all drive
//! this same engine.
//! 本 crate 是可复用的核心：分析器、压缩器、设置规范化、线上传输模型和错误类型
//! 都在这里。Tauri 应用、criterion 基准、pdf-compressor-cli 与 cargo-fuzz 驱动同一引擎。
//!
//! Feature `specta`: derives `specta::Type` on the wire models so the desktop
//! app can generate TypeScript IPC bindings.
//! specta 特性：为线上传输模型派生 specta::Type，供桌面应用生成 TS IPC 绑定。

pub mod error;
pub mod models;
pub mod pdf;
pub mod quick_profile;
#[cfg(feature = "testutil")]
pub mod testutil;

pub use error::{AppError, AppErrorPayload};
pub use models::CompressionResponse;
pub use pdf::{
    analyze_pdf_with_progress, compress_pdf_to_target_size, compress_pdf_with_progress,
    BilevelCodec, CompressionSettings, CompressionSettingsOverrides,
};
