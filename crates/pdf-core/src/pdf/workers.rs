//! Bounded scoped worker pool shared by the image optimizer and the
//! target-size probe rounds.
//! 有界作用域线程池 — 图片优化与目标大小探测轮共用。

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

use super::ensure_not_cancelled;
use crate::error::AppError;

/// Worker channel buffer multiplier relative to worker count.
/// Each queued task carries its full stream bytes, so a small multiplier keeps
/// the in-flight backlog bounded without blocking the producer.
pub(crate) const CHANNEL_BUFFER_MULTIPLIER: usize = 2;

/// Distribute `tasks` over `worker_count` scoped threads.
///
/// Tasks move into a bounded channel (`buffer` slots); workers apply `work`
/// and send results back in completion order; `on_result` runs on the calling
/// thread, which stays the only thread touching shared state such as the
/// document. The pool owns cancellation: workers exit when the flag flips,
/// and the feed/collect loops abort with `AppError::Cancelled`. The first
/// `on_result` error aborts collection and is returned; worker-side errors
/// travel inside the result payload and are unpacked by the caller.
pub(crate) fn run_worker_pool<T, R, F, G>(
    tasks: Vec<T>,
    worker_count: usize,
    buffer: usize,
    cancel_flag: &Arc<AtomicBool>,
    task_id: &str,
    work: F,
    mut on_result: G,
) -> Result<(), AppError>
where
    T: Send,
    R: Send,
    F: Fn(T) -> R + Sync,
    G: FnMut(R) -> Result<(), AppError>,
{
    let expected = tasks.len();
    if expected == 0 {
        return Ok(());
    }

    thread::scope(|scope| -> Result<(), AppError> {
        let (task_tx, task_rx) = mpsc::sync_channel::<T>(buffer.max(1));
        let task_rx = Arc::new(Mutex::new(task_rx));
        let (result_tx, result_rx) = mpsc::channel::<R>();

        // Spawn worker threads. `work` is shared by reference: every capture
        // it holds (settings, cancel flag, task id) is Sync by construction.
        for _ in 0..worker_count.max(1) {
            let rx = Arc::clone(&task_rx);
            let tx = result_tx.clone();
            let work = &work;
            scope.spawn(move || loop {
                if cancel_flag.load(Ordering::Relaxed) {
                    return;
                }
                let task = match rx.lock() {
                    Ok(guard) => guard.recv(),
                    Err(_) => return,
                };
                let Ok(task) = task else { return };
                if tx.send(work(task)).is_err() {
                    return;
                }
            });
        }

        // Drop the spare sender so result_rx closes when all workers finish.
        drop(result_tx);

        // Feed tasks into the channel. A blocking `send` could never return
        // under cancellation (the scope keeps a receiver alive, so the
        // channel never disconnects while we wait), so the feed polls with
        // `try_send` and re-checks the flag — cancellation unblocks the
        // producer, whose early return drops the sender and releases the
        // workers.
        for task in tasks {
            let mut task = task;
            loop {
                ensure_not_cancelled(cancel_flag, task_id)?;
                match task_tx.try_send(task) {
                    Ok(()) => break,
                    Err(mpsc::TrySendError::Full(returned)) => {
                        task = returned;
                        // Workers are saturating the channel; a short park
                        // keeps the poll cheap while progress is being made.
                        thread::park_timeout(Duration::from_millis(2));
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => {
                        return Err(AppError::PdfBuild(
                            "A worker exited before its tasks were scheduled.".to_string(),
                        ));
                    }
                }
            }
        }
        drop(task_tx);

        // Collect results; shared state is only touched on this thread.
        for _ in 0..expected {
            ensure_not_cancelled(cancel_flag, task_id)?;
            let result = result_rx.recv().map_err(|_| {
                AppError::PdfBuild("A worker exited before returning its result.".to_string())
            })?;
            on_result(result)?;
        }

        Ok(())
    })?;

    Ok(())
}
