#![no_main]

//! Fuzz target for the bytes pipeline (CLI stdin/stdout mode): the
//! `compress_pdf_bytes_with_progress` entry that pipes untrusted bytes with
//! no filesystem round-trip, plus its target-size twin. The file-path
//! pipeline target (`pipeline.rs`) shares the engine but exercises different
//! I/O plumbing; the public bytes API previously had zero fuzz coverage.
//!
//! Run from `crates/pdf-core/` with:
//! `cargo +nightly fuzz run bytes -- -max_total_time=60`

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use libfuzzer_sys::fuzz_target;

use pdf_core::{
    compress_pdf_bytes_to_target_size, compress_pdf_bytes_with_progress,
    CompressionSettings, CompressionSettingsOverrides,
};

fuzz_target!(|data: &[u8]| {
    // The bytes pipeline must tolerate arbitrary input without panicking and
    // must always hand back some bytes (never run a pipe dry).
    let settings = CompressionSettings::from_sources(
        None,
        CompressionSettingsOverrides {
            subset_fonts: Some(true),
            cmyk_conversion: Some(true),
            ..Default::default()
        },
    );
    let cancel = Arc::new(AtomicBool::new(false));

    if let Ok(outcome) =
        compress_pdf_bytes_with_progress(data.to_vec(), None, settings.clone(), cancel.clone(), |_| {})
    {
        debug_assert!(!outcome.bytes.is_empty(), "the pipe must never run dry");
    }

    // The target-size bytes path adds the search schedule and the
    // passthrough accounting on top.
    let _ = compress_pdf_bytes_to_target_size(
        data.to_vec(),
        None,
        64 * 1024,
        settings,
        cancel,
        &mut |_| {},
    );
});
