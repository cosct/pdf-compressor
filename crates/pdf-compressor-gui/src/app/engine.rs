//! Direct blocking calls into pdf-core. The UI/scheduler owns the cancel flag
//! and runs these functions on GPUI's background executor; callbacks carry
//! progress to the entity through the scheduler's channel.

use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

use pdf_core::{
    AppError, analyze_pdf_with_progress, compress_pdf_to_target_size, compress_pdf_with_progress,
    models::{AnalysisResponse, CompressionResponse, ProgressUpdate},
};

use super::settings::Settings;

pub fn analyze(
    path: &Path,
    password: Option<&str>,
    settings: &Settings,
    cancel: &Arc<AtomicBool>,
    mut on_progress: impl FnMut(ProgressUpdate),
) -> Result<AnalysisResponse, AppError> {
    let path = path.to_string_lossy();
    let engine_settings = settings.to_engine(false);
    analyze_pdf_with_progress(&path, password, Some(&engine_settings), cancel, |update| {
        on_progress(update)
    })
}

pub fn compress(
    path: &Path,
    password: Option<&str>,
    settings: &Settings,
    scanned: bool,
    cancel: Arc<AtomicBool>,
    mut on_progress: impl FnMut(ProgressUpdate),
) -> Result<CompressionResponse, AppError> {
    let path = path.to_string_lossy().into_owned();
    let engine_settings = settings.to_engine(scanned);
    match settings.target_size_bytes() {
        Some(target_bytes) => compress_pdf_to_target_size(
            &path,
            password,
            target_bytes,
            engine_settings,
            cancel,
            &mut on_progress,
        ),
        None => compress_pdf_with_progress(&path, password, engine_settings, cancel, on_progress),
    }
}
