//! Mobile error mirror — 8 variants covering the engine's 11-code taxonomy.
//! 移动端错误镜像 — 8 个变体覆盖引擎 11 码分类。
//!
//! Dropped are the desktop-shell codes (`error.io`, `error.opener`,
//! `error.config`): the bytes pipeline has no filesystem, no system opener,
//! and the only Config source (zero target size) reaches the caller as
//! [`FfiError::Engine`] detail. [`FfiError::InvalidPdf`] replaces
//! `error.invalidPdfPath` — mobile callers hand over bytes from the SAF, so
//! there is no path to report; the ffi layer classifies non-PDF bytes up
//! front (header sniff) instead of relying on the engine's extension check.
//! Every variant's `code()` is one of the engine's closed `error.*` set, so
//! the Android `strings.xml` stays key-compatible with the desktop locales
//! (pinned by the parity test against the committed strings.xml).

use pdf_core::AppError;

/// Mirrors the engine's `error.*` i18n codes: the string the Kotlin layer
/// looks up in `strings.xml`. `Engine` reports `error.pdfBuild` — the
/// dominant collapsed source; the original detail rides in `detail`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Enum)]
pub enum FfiError {
    #[error("The selected file does not exist")]
    MissingInput,
    #[error("The selected file is not a PDF")]
    InvalidPdf,
    #[error(
        "The selected file is too large to process on this device ({size_bytes} bytes; the limit is {limit_bytes} bytes)"
    )]
    InputTooLarge { size_bytes: u64, limit_bytes: u64 },
    #[error("The PDF is DRM-encrypted with an unsupported security handler")]
    EncryptedPdf,
    #[error("The PDF requires an open password; supply one to process the file")]
    PasswordRequired,
    #[error("The supplied password did not unlock the PDF")]
    WrongPassword,
    #[error("The operation was cancelled")]
    Cancelled,
    #[error("The engine failed: {detail}")]
    Engine { detail: String },
}

impl FfiError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingInput => "error.missingInput",
            Self::InvalidPdf => "error.invalidPdfPath",
            Self::InputTooLarge { .. } => "error.inputTooLarge",
            Self::EncryptedPdf => "error.encryptedPdf",
            Self::PasswordRequired => "error.passwordRequired",
            Self::WrongPassword => "error.wrongPassword",
            Self::Cancelled => "error.cancelled",
            Self::Engine { .. } => "error.pdfBuild",
        }
    }
}

impl From<AppError> for FfiError {
    fn from(error: AppError) -> Self {
        match error {
            AppError::MissingInput(_) => Self::MissingInput,
            AppError::InvalidPdfPath(_) => Self::InvalidPdf,
            AppError::InputTooLarge {
                size_bytes,
                limit_bytes,
            } => Self::InputTooLarge {
                size_bytes,
                limit_bytes,
            },
            AppError::Encrypted => Self::EncryptedPdf,
            AppError::PasswordRequired => Self::PasswordRequired,
            AppError::WrongPassword => Self::WrongPassword,
            AppError::Cancelled(_) => Self::Cancelled,
            AppError::PdfBuild(detail) => Self::Engine { detail },
            AppError::Io(error) => Self::Engine {
                detail: error.to_string(),
            },
            AppError::Config(detail) => Self::Engine { detail },
            AppError::Opener(detail) => Self::Engine { detail },
        }
    }
}
