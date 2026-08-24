#![no_main]

//! Coverage-guided fuzz target for the PDF analysis/compression pipeline.
//! PDF 分析/压缩管线的覆盖率引导模糊测试目标。
//!
//! The application opens untrusted files, so the whole pipeline must never
//! panic on malformed input. Run from `crates/pdf-core/` with:
//! `cargo +nightly fuzz run pipeline -- -max_total_time=60`

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use libfuzzer_sys::fuzz_target;

use pdf_core::{
    analyze_pdf_with_progress, compress_pdf_to_target_size, compress_pdf_with_progress,
    CompressionSettings, CompressionSettingsOverrides,
};

fuzz_target!(|data: &[u8]| {
    let dir = std::env::temp_dir().join(format!("pdf-core-fuzz-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("input.pdf");
    if std::fs::write(&path, data).is_err() {
        return;
    }
    let path_str = path.to_string_lossy().into_owned();

    let _ = analyze_pdf_with_progress(&path_str, |_| {});

    let settings = CompressionSettings::from_sources(None, CompressionSettingsOverrides::default());
    let _ = compress_pdf_with_progress(
        &path_str,
        settings.clone(),
        Arc::new(AtomicBool::new(false)),
        |_| {},
    );

    // The target-size search is a separate hot path (in-memory probing,
    // binary quality/edge schedule, materialize + verify) — fuzz it too.
    // The modest budget keeps valid inputs inside a few probe rounds.
    let _ = compress_pdf_to_target_size(
        &path_str,
        64 * 1024,
        settings,
        Arc::new(AtomicBool::new(false)),
        &mut |_| {},
    );

    // Keep the scratch directory empty: output naming deduplicates, so
    // leftover files would change behavior across iterations.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
});
