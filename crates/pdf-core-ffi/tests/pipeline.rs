//! Pipeline pins: run the ffi entries against real engine fixtures on the
//! host — progress ordering, cancellation, passthrough semantics, preflight.
//! 管线钉子：宿主上用引擎夹具直跑 ffi 入口 —— 进度顺序、取消、直通语义、预检。

use std::sync::{Arc, Mutex};

use pdf_core::testutil::jpeg_page_pdf_bytes;
use pdf_core_ffi::{
    analyze, compress, compress_to_target, ping, FfiCancelHandle, FfiError, FfiProgress,
    FfiProgressUpdate, FfiSettings,
};

/// Shared progress log: the handle outlives the boxed callback handed to
/// the ffi layer, so assertions can read the recording after the run.
#[derive(Clone, Default)]
#[allow(clippy::type_complexity)] // a one-shot cancel hook; not worth a type alias
struct Recording {
    updates: Arc<Mutex<Vec<FfiProgressUpdate>>>,
    /// Fired on the first callback — the mid-run cancel hook.
    on_first: Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>,
}

impl Recording {
    fn callback(&self) -> Box<dyn FfiProgress> {
        Box::new(Recorder(self.clone()))
    }

    fn snapshots(&self) -> Vec<(String, f32)> {
        self.updates
            .lock()
            .unwrap()
            .iter()
            .map(|update| (update.phase.clone(), update.percent))
            .collect()
    }

    fn cancel_on_first_progress(&self, handle: &Arc<FfiCancelHandle>) {
        let handle = Arc::clone(handle);
        *self.on_first.lock().unwrap() = Some(Box::new(move || handle.cancel()));
    }
}

struct Recorder(Recording);

impl FfiProgress for Recorder {
    fn on_progress(&self, update: FfiProgressUpdate) {
        if let Some(hook) = self.0.on_first.lock().unwrap().take() {
            hook();
        }
        self.0.updates.lock().unwrap().push(update);
    }
}

fn fixture_bytes() -> Vec<u8> {
    // A fat high-quality JPEG page: big enough for the balanced preset to
    // beat, exercising the real decode → re-encode → serialize path.
    jpeg_page_pdf_bytes(1600, 2000, 95)
}

// ---------------------------------------------------------------------------
// Progress ordering
// ---------------------------------------------------------------------------

#[test]
fn compress_progress_starts_low_ends_done_at_100() {
    let recording = Recording::default();
    let result = compress(
        fixture_bytes(),
        None,
        FfiSettings::default(),
        FfiCancelHandle::new(),
        recording.callback(),
    )
    .expect("fixture must compress");

    let snapshots = recording.snapshots();
    assert!(
        snapshots.len() >= 2,
        "expected multiple throttled updates, got {snapshots:?}"
    );
    assert_eq!(snapshots.first().unwrap().0, "compressing");
    assert_eq!(snapshots.last().unwrap().0, "done");
    assert_eq!(snapshots.last().unwrap().1, 100.0);
    // Monotone within the compressing phase; the done phase resets to 100.
    let compressing: Vec<f32> = snapshots
        .iter()
        .take_while(|(phase, _)| phase == "compressing")
        .map(|(_, percent)| *percent)
        .collect();
    let mut sorted = compressing.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(compressing, sorted, "percent must never regress mid-run");
    assert!(
        result.bytes.starts_with(b"%PDF"),
        "output bytes must be a valid PDF header"
    );
}

#[test]
fn analyze_progress_reports_analyzing_then_done() {
    let recording = Recording::default();
    let analysis = analyze(
        fixture_bytes(),
        None,
        None,
        FfiCancelHandle::new(),
        recording.callback(),
    )
    .expect("fixture must analyze");

    let snapshots = recording.snapshots();
    assert_eq!(snapshots.first().unwrap().0, "analyzing");
    assert_eq!(snapshots.last().unwrap().0, "done");

    assert_eq!(analysis.page_count, 1);
    assert_eq!(analysis.image_object_count, 1);
    assert_eq!(analysis.file_size_bytes, fixture_bytes().len() as u64);
    assert!(["text-native", "mixed", "scan-heavy"].contains(&analysis.document_kind.as_str()));
    assert!(["conservative", "balanced", "maximum"].contains(&analysis.recommended_preset.as_str()));
}

// ---------------------------------------------------------------------------
// Result semantics
// ---------------------------------------------------------------------------

#[test]
fn compress_result_fields_stay_self_consistent() {
    let result = compress(
        fixture_bytes(),
        None,
        FfiSettings::default(),
        FfiCancelHandle::new(),
        Recording::default().callback(),
    )
    .expect("fixture must compress");

    assert_eq!(result.original_size_bytes as usize, fixture_bytes().len());
    assert_eq!(
        result.compressed_size_bytes as usize,
        result.bytes.len(),
        "compressed_size_bytes must describe the returned bytes"
    );
    let saved = result.original_size_bytes as i64 - result.compressed_size_bytes as i64;
    assert_eq!(saved, result.saved_bytes as i64);
    if result.output_was_smaller {
        assert!(result.compressed_size_bytes < result.original_size_bytes);
        assert!(result.savings_percent > 0.0);
    } else {
        // The pipe's passthrough rule: hand back the original untouched.
        assert_eq!(result.compressed_size_bytes, result.original_size_bytes);
    }
}

#[test]
fn compress_to_target_meets_a_generous_budget_or_warns() {
    let result = compress_to_target(
        fixture_bytes(),
        None,
        4 * 1024 * 1024,
        FfiSettings::default(),
        FfiCancelHandle::new(),
        Recording::default().callback(),
    )
    .expect("target search must complete");

    if result.output_was_smaller && result.compressed_size_bytes > 4 * 1024 * 1024 {
        let warned = result
            .notices
            .iter()
            .any(|notice| notice.level == "warning" || notice.level == "error");
        assert!(
            warned,
            "a missed budget must carry a warning notice, got {:?}",
            result.notices
        );
    }
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

#[test]
fn precancelled_handles_fail_fast_with_cancelled() {
    let bytes = fixture_bytes();
    for (label, run) in [
        (
            "analyze",
            Box::new(|| {
                analyze(
                    bytes.clone(),
                    None,
                    None,
                    cancelled_handle(),
                    Recording::default().callback(),
                )
                .map(|_| ())
            }) as Box<dyn FnOnce() -> Result<(), FfiError>>,
        ),
        (
            "compress",
            Box::new(|| {
                compress(
                    bytes.clone(),
                    None,
                    FfiSettings::default(),
                    cancelled_handle(),
                    Recording::default().callback(),
                )
                .map(|_| ())
            }),
        ),
        (
            "compress_to_target",
            Box::new(|| {
                compress_to_target(
                    bytes.clone(),
                    None,
                    1024,
                    FfiSettings::default(),
                    cancelled_handle(),
                    Recording::default().callback(),
                )
                .map(|_| ())
            }),
        ),
    ] {
        let error = run().expect_err("pre-cancelled call must fail");
        assert!(
            matches!(error, FfiError::Cancelled),
            "{label} must fail fast with Cancelled, got {error:?}"
        );
    }
}

fn cancelled_handle() -> Arc<FfiCancelHandle> {
    let handle = FfiCancelHandle::new();
    handle.cancel();
    handle
}

#[test]
fn cancelling_from_the_progress_callback_aborts_the_run() {
    let handle = FfiCancelHandle::new();
    let cancel_hook_handle = Arc::clone(&handle);
    let recording = Recording::default();
    recording.cancel_on_first_progress(&cancel_hook_handle);

    let error = compress(
        fixture_bytes(),
        None,
        FfiSettings::default(),
        handle,
        recording.callback(),
    )
    .expect_err("mid-run cancel must abort");
    assert!(matches!(error, FfiError::Cancelled));
}

// ---------------------------------------------------------------------------
// Preflight: sniff + size ceiling
// ---------------------------------------------------------------------------

#[test]
fn garbage_bytes_classify_as_invalid_pdf_not_engine_failure() {
    let error = compress(
        b"clearly not a pdf".to_vec(),
        None,
        FfiSettings::default(),
        FfiCancelHandle::new(),
        Recording::default().callback(),
    )
    .expect_err("garbage must be rejected");
    assert!(matches!(error, FfiError::InvalidPdf));

    let error = analyze(
        Vec::new(),
        None,
        None,
        FfiCancelHandle::new(),
        Recording::default().callback(),
    )
    .expect_err("empty bytes must be rejected");
    assert!(matches!(error, FfiError::InvalidPdf));
}

#[test]
fn junk_before_the_pdf_header_still_passes_the_sniff() {
    // PDFs may carry leading junk within lopdf's 1024-byte header search
    // window; the sniff must not reject what the engine accepts.
    let mut bytes = vec![0x42u8; 512];
    bytes.extend_from_slice(&fixture_bytes());
    analyze(
        bytes,
        None,
        None,
        FfiCancelHandle::new(),
        Recording::default().callback(),
    )
    .expect("junk-prefixed PDF must analyze");
}

#[test]
fn ping_carries_the_crate_version() {
    let ping = ping();
    assert!(ping.starts_with("pdf-core-ffi "), "got {ping}");
    assert!(ping.ends_with(" ok"));
}
