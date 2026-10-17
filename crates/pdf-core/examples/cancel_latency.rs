//! Cancel-latency probe for the benchmark harness (docs/BENCHMARK.md).
//! Drives the public bytes API with a watchdog cancel and prints one JSON
//! object per run: cancel time, total time, and the outcome.
//!
//! usage: cancel_latency <input.pdf> [cancel_after_ms] [runs]

use pdf_core::{
    compress_pdf_bytes_with_progress, AppError, CompressionSettings, CompressionSettingsOverrides,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .expect("usage: cancel_latency <input.pdf> [cancel_after_ms] [runs]");
    let cancel_after = args
        .get(2)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(250);
    let runs = args
        .get(3)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(3);
    let bytes = std::fs::read(path).expect("read input");

    for run in 0..runs {
        let flag = Arc::new(AtomicBool::new(false));
        let watchdog = {
            let flag = flag.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(cancel_after));
                flag.store(true, Ordering::SeqCst);
                Instant::now()
            })
        };
        let settings =
            CompressionSettings::from_sources(None, CompressionSettingsOverrides::default());
        let start = Instant::now();
        let result = compress_pdf_bytes_with_progress(bytes.clone(), None, settings, flag, |_| {});
        let returned_at = Instant::now();
        let total_ms = (returned_at - start).as_secs_f64() * 1000.0;
        let cancelled_at = watchdog.join().expect("watchdog thread");
        let outcome = match &result {
            Err(AppError::Cancelled(_)) => "cancelled",
            Err(_) => "error",
            Ok(_) => "completed",
        };
        // Null when the run finished before the watchdog fired.
        let latency_ms = (returned_at >= cancelled_at)
            .then(|| (returned_at - cancelled_at).as_secs_f64() * 1000.0);
        println!(
            "{{\"run\":{run},\"cancel_after_ms\":{cancel_after},\"total_ms\":{total_ms:.1},\"latency_ms\":{latency},\"outcome\":\"{outcome}\"}}",
            latency = latency_ms.map_or("null".into(), |value| format!("{value:.1}")),
        );
    }
}
