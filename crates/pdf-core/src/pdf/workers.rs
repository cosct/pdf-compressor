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
/// Each queued result may retain stream bytes and caches; keep the backlog
/// bounded while the caller consumes results concurrently.
pub(crate) const CHANNEL_BUFFER_MULTIPLIER: usize = 2;

/// Distribute `tasks` over `worker_count` scoped threads.
///
/// Workers pull owned tasks and apply `work`; a bounded result channel (`buffer` slots)
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
    let tasks = Mutex::new(tasks.into_iter());
    thread::scope(|scope| -> Result<(), AppError> {
        // Workers pull tasks directly. The caller drains results while they
        // run, so bounded results cannot deadlock behind a blocked producer.
        let (tx, rx) = mpsc::sync_channel(buffer.max(1));
        for _ in 0..worker_count.max(1) {
            let tx = tx.clone();
            let tasks = &tasks;
            let work = &work;
            scope.spawn(move || loop {
                if cancel_flag.load(Ordering::Relaxed) {
                    return;
                }
                let task = tasks.lock().unwrap_or_else(|e| e.into_inner()).next();
                let Some(task) = task else {
                    return;
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(task)))
                    .map_err(|_| {
                        AppError::PdfBuild("A worker task panicked; the run was aborted.".into())
                    });
                if tx.send(result).is_err() {
                    return;
                }
            });
        }
        drop(tx);
        for _ in 0..expected {
            loop {
                ensure_not_cancelled(cancel_flag, task_id)?;
                match rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(result) => {
                        on_result(result?)?;
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(AppError::PdfBuild(
                            "A worker exited before returning its result.".into(),
                        ))
                    }
                }
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_results_stop_production_when_consumer_fails() {
        use std::sync::atomic::AtomicUsize;
        let cancel = Arc::new(AtomicBool::new(false));
        let started = AtomicUsize::new(0);
        let result = run_worker_pool(
            (0..1000).collect(),
            2,
            1,
            &cancel,
            "failure",
            |n| {
                started.fetch_add(1, Ordering::SeqCst);
                n
            },
            |_| Err(AppError::Config("stop".into())),
        );
        assert!(result.is_err());
        // One consumed result + one buffered result + one held per worker,
        // plus each worker's final failed send after the receiver closes.
        assert!(started.load(Ordering::SeqCst) <= 6);
    }

    #[test]
    fn consumer_cancellation_releases_blocked_workers() {
        let cancel = Arc::new(AtomicBool::new(false));
        let result = run_worker_pool(
            (0..1000).collect(),
            3,
            1,
            &cancel,
            "cancel",
            |n| n,
            |_| {
                cancel.store(true, Ordering::Relaxed);
                Ok(())
            },
        );
        assert!(matches!(result, Err(AppError::Cancelled(_))));
    }

    #[test]
    fn panicked_task_surfaces_as_an_error_without_killing_the_process() {
        let cancel = Arc::new(AtomicBool::new(false));
        let error = run_worker_pool(
            vec![1u8, 2, 3, 4],
            2,
            4,
            &cancel,
            "test",
            |task| {
                if task == 2 {
                    panic!("boom");
                }
                task
            },
            |_| Ok(()),
        )
        .expect_err("a panicked task must abort the pool with an error");
        assert!(matches!(error, AppError::PdfBuild(message) if message.contains("panicked")));
    }

    #[test]
    fn pool_completes_normally_without_panics() {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut seen = 0usize;
        run_worker_pool(
            vec![1u8, 2, 3],
            2,
            4,
            &cancel,
            "test",
            |task| task * 10,
            |result| {
                seen += result as usize;
                Ok(())
            },
        )
        .expect("healthy pool runs");
        assert_eq!(seen, 60);
    }
}
