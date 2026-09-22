//! Exported FFI surface — the five mobile entry points plus the progress
//! callback interface. Blocking by design: Rust does the work on the
//! calling thread; Kotlin wraps in `withContext(Dispatchers.IO)` and wires
//! the cancel handle to `invokeOnCancellation` (no UniFFI async dependency,
//! per the plan's threading decision).
//!
//! Every entry shares the same preflight: the 512 MiB mobile input
//! pre-check (friendlier than the engine's 2 GiB hard limit on
//! low-memory devices) and the PDF header sniff (bytes without a `%PDF-`
//! header classify as [`FfiError::InvalidPdf`] instead of a generic
//! engine parse failure — SAF mime filtering is advisory, not a guarantee).

use std::sync::Arc;

use pdf_core::models::{CompressionSettingsPayload, ProgressUpdate};
use pdf_core::{
    analyze_pdf_bytes_with_progress, compress_pdf_bytes_to_target_size,
    compress_pdf_bytes_with_progress, CompressionPreset, CompressionSettings,
    CompressionSettingsOverrides,
};

use crate::cancel::FfiCancelHandle;
use crate::dto::{
    FfiAnalysis, FfiBuildFeatures, FfiCompressResult, FfiNotice, FfiPresetProfile, FfiSettings,
};
use crate::error::FfiError;

/// Mobile input ceiling: 512 MiB. The engine's own limit is 2 GiB; the ffi
/// layer reports the friendlier, device-appropriate bound up front because
/// peak memory on decode + serialize runs ≈2× input.
pub const MAX_MOBILE_INPUT_BYTES: u64 = 512 * 1024 * 1024;

/// Exposed so Kotlin can show the limit without hardcoding it.
#[uniffi::export]
pub fn max_mobile_input_bytes() -> u64 {
    MAX_MOBILE_INPUT_BYTES
}

/// Liveness probe for the spike (S2): the string the skeleton app shows.
#[uniffi::export]
pub fn ping() -> String {
    format!("pdf-core-ffi {} ok", env!("CARGO_PKG_VERSION"))
}

/// One progress event crossing the FFI: the engine's `ProgressUpdate`
/// verbatim (phase string, percent, optional notice).
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiProgressUpdate {
    pub phase: String,
    pub percent: f32,
    pub message: Option<FfiNotice>,
}

/// Progress callback implemented on the Kotlin side. The engine already
/// throttles to 3% granularity — no extra layer here. Callbacks arrive on
/// whatever thread the engine reports from (its worker pool); UI updates
/// must hop to the main dispatcher on the Kotlin side.
#[uniffi::export(callback_interface)]
pub trait FfiProgress: Send + Sync {
    fn on_progress(&self, update: FfiProgressUpdate);
}

/// Adapt a boxed callback into the engine's `FnMut(ProgressUpdate)`.
fn progress_reporter(progress: Box<dyn FfiProgress>) -> impl FnMut(ProgressUpdate) {
    move |update: ProgressUpdate| {
        progress.on_progress(FfiProgressUpdate {
            phase: update.phase,
            percent: update.percent,
            message: update.message.map(FfiNotice::from),
        });
    }
}

/// Mobile preflight shared by every byte entry: size ceiling + PDF sniff.
fn preflight(bytes: &[u8]) -> Result<(), FfiError> {
    if bytes.len() as u64 > MAX_MOBILE_INPUT_BYTES {
        return Err(FfiError::InputTooLarge {
            size_bytes: bytes.len() as u64,
            limit_bytes: MAX_MOBILE_INPUT_BYTES,
        });
    }
    // PDFs may carry junk before the header (lopdf searches the first
    // 1024 bytes); outside that window the engine load would fail anyway.
    let window = &bytes[..bytes.len().min(1024)];
    if !window.windows(5).any(|candidate| candidate == b"%PDF-") {
        return Err(FfiError::InvalidPdf);
    }
    Ok(())
}

fn resolve_settings(settings: FfiSettings) -> CompressionSettings {
    let payload: Option<CompressionSettingsPayload> = Some(settings.to_payload());
    CompressionSettings::from_sources(payload, CompressionSettingsOverrides::default())
}

/// Analyze a PDF held in memory: classification, estimate, recommended
/// preset — same semantics as the desktop's analyze command. `settings` is
/// the optional caller context (job's live settings); `None` analyzes
/// under the default posture. Cancel and password semantics mirror
/// [`compress`].
#[uniffi::export]
pub fn analyze(
    bytes: Vec<u8>,
    password: Option<String>,
    settings: Option<FfiSettings>,
    cancel: Arc<FfiCancelHandle>,
    progress: Box<dyn FfiProgress>,
) -> Result<FfiAnalysis, FfiError> {
    preflight(&bytes)?;
    let engine_settings = settings.map(resolve_settings);
    analyze_pdf_bytes_with_progress(
        &bytes,
        password.as_deref(),
        engine_settings.as_ref(),
        &cancel.flag(),
        progress_reporter(progress),
    )
    .map(FfiAnalysis::from)
    .map_err(FfiError::from)
}

/// Compress a PDF held in memory and return the output bytes plus the
/// standard response. The pipe's passthrough rule applies: when the engine
/// cannot beat the input, `bytes` carries the original and
/// `output_was_smaller` is false — Kotlin must check that flag before
/// offering a save (the desktop shows the same honesty).
#[uniffi::export]
pub fn compress(
    bytes: Vec<u8>,
    password: Option<String>,
    settings: FfiSettings,
    cancel: Arc<FfiCancelHandle>,
    progress: Box<dyn FfiProgress>,
) -> Result<FfiCompressResult, FfiError> {
    preflight(&bytes)?;
    compress_pdf_bytes_with_progress(
        bytes,
        password.as_deref(),
        resolve_settings(settings),
        cancel.flag(),
        progress_reporter(progress),
    )
    .map(FfiCompressResult::from)
    .map_err(FfiError::from)
}

/// Target-size search: probe quality/edge parameters until the output
/// fits `target_bytes` (best effort — a budget that cannot be met returns
/// the smallest achievable result with a warning notice). Same passthrough
/// and password semantics as [`compress`].
#[uniffi::export]
pub fn compress_to_target(
    bytes: Vec<u8>,
    password: Option<String>,
    target_bytes: u64,
    settings: FfiSettings,
    cancel: Arc<FfiCancelHandle>,
    progress: Box<dyn FfiProgress>,
) -> Result<FfiCompressResult, FfiError> {
    preflight(&bytes)?;
    let mut reporter = progress_reporter(progress);
    compress_pdf_bytes_to_target_size(
        bytes,
        password.as_deref(),
        target_bytes,
        resolve_settings(settings),
        cancel.flag(),
        &mut reporter,
    )
    .map(FfiCompressResult::from)
    .map_err(FfiError::from)
}

/// The preset table, read straight from the engine's `CompressionPreset`
/// (the single source of truth — same values the desktop panel and CLI
/// use). Three rows: conservative / balanced / maximum.
#[uniffi::export]
pub fn preset_defaults() -> Vec<FfiPresetProfile> {
    [
        CompressionPreset::Conservative,
        CompressionPreset::Balanced,
        CompressionPreset::Maximum,
    ]
    .into_iter()
    .map(|preset| FfiPresetProfile {
        name: preset.as_label().to_string(),
        image_quality: preset.default_quality(),
        max_image_size_percent: preset.default_max_image_size_percent(),
        max_image_size_px: preset.default_max_image_size_px(),
        strip_metadata: preset.default_strip_metadata(),
        subset_fonts: preset.default_subset_fonts(),
    })
    .collect()
}

/// What this build's engine can actually do (feature flags surfaced
/// honestly — e.g. a `cmyk-cms`-less build keeps CMYK images untouched).
#[uniffi::export]
pub fn build_features() -> FfiBuildFeatures {
    pdf_core::build_features().into()
}
