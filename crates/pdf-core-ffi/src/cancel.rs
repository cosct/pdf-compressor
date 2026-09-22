//! Cooperative cancellation handle — the mobile twin of the engine's
//! `Arc<AtomicBool>` cancel flag. Kotlin constructs one per job, keeps the
//! reference for the cancel button, and the ffi layer feeds the flag into
//! every engine call (including analyze, symmetric with compress per the
//! 0.11.0 cancel-parameter contract).

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Opaque cancel token. `cancel()` is idempotent and safe from any thread;
/// an already-cancelled handle makes every engine entry fail fast with
/// [`crate::FfiError::Cancelled`] (the engine checks the flag at its own
/// phase boundaries and inside the image worker loop).
#[derive(uniffi::Object)]
pub struct FfiCancelHandle {
    flag: Arc<AtomicBool>,
}

#[uniffi::export]
impl FfiCancelHandle {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            flag: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Request cancellation. Takes effect at the engine's next check —
    /// long decodes are interruptible between images, not mid-image.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }
}

impl FfiCancelHandle {
    /// The engine-facing flag (cloned `Arc`; cheap).
    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.flag)
    }
}
