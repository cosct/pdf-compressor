use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use super::job::JobId;

#[derive(Debug, Clone)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.0)
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
pub struct Scheduler {
    analysis_queue: VecDeque<JobId>,
    analysis_active: Option<JobId>,
    compression_active: bool,
    cancellation_in_flight: bool,
    active_tokens: Vec<(JobId, CancelToken)>,
}

impl Scheduler {
    pub fn queue_analysis(&mut self, id: JobId) -> bool {
        if self.analysis_active == Some(id) || self.analysis_queue.contains(&id) {
            return false;
        }
        self.analysis_queue.push_back(id);
        true
    }

    pub fn pop_analysis(&mut self) -> Option<JobId> {
        if self.analysis_active.is_some() || self.cancellation_in_flight {
            return None;
        }
        let next = self.analysis_queue.pop_front()?;
        self.analysis_active = Some(next);
        Some(next)
    }

    pub fn finish_analysis(&mut self, id: JobId) {
        if self.analysis_active == Some(id) {
            self.analysis_active = None;
        }
    }

    pub fn clear_queued_analysis(&mut self) {
        self.analysis_queue.clear();
    }

    pub fn has_pending_analysis(&self) -> bool {
        self.analysis_active.is_some() || !self.analysis_queue.is_empty()
    }

    pub fn begin_compression(&mut self) -> bool {
        if self.compression_active || self.cancellation_in_flight {
            return false;
        }
        self.compression_active = true;
        true
    }

    pub fn finish_compression(&mut self) {
        self.compression_active = false;
    }

    pub fn register_token(&mut self, id: JobId, token: CancelToken) {
        self.active_tokens.push((id, token));
    }

    pub fn unregister_token(&mut self, id: JobId, token: &CancelToken) {
        self.active_tokens
            .retain(|(job_id, active)| *job_id != id || !Arc::ptr_eq(&active.0, &token.0));
    }

    /// Mark cancellation now, cancel every currently active core call, and
    /// hold the scheduler lock until the caller observes all worker joins.
    pub fn begin_cancel(&mut self) -> Vec<(JobId, CancelToken)> {
        self.cancellation_in_flight = true;
        self.clear_queued_analysis();
        for (_, token) in &self.active_tokens {
            token.cancel();
        }
        self.active_tokens.clone()
    }

    pub fn finish_cancel(&mut self) {
        self.cancellation_in_flight = false;
        self.analysis_active = None;
        self.compression_active = false;
        self.active_tokens.clear();
    }

    pub fn is_locked(&self) -> bool {
        self.compression_active || self.cancellation_in_flight
    }

    pub fn analysis_running(&self) -> bool {
        self.analysis_active.is_some()
    }

    pub fn compression_running(&self) -> bool {
        self.compression_active
    }

    pub fn cancellation_in_flight(&self) -> bool {
        self.cancellation_in_flight
    }
}

pub fn compression_concurrency(job_count: usize, logical_cpus: usize) -> usize {
    if job_count <= 1 {
        return job_count;
    }
    let cpu = if logical_cpus == 0 { 4 } else { logical_cpus };
    2_usize.min((cpu / 2).max(1)).min(job_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_is_fifo_serial_and_suppresses_duplicate_waiters() {
        let mut scheduler = Scheduler::default();
        assert!(scheduler.queue_analysis(JobId(1)));
        assert!(!scheduler.queue_analysis(JobId(1)));
        assert!(scheduler.queue_analysis(JobId(2)));
        assert_eq!(scheduler.pop_analysis(), Some(JobId(1)));
        assert_eq!(scheduler.pop_analysis(), None);
        scheduler.finish_analysis(JobId(1));
        assert_eq!(scheduler.pop_analysis(), Some(JobId(2)));
    }

    #[test]
    fn cancellation_sets_flags_and_keeps_lock_until_workers_settle() {
        let mut scheduler = Scheduler::default();
        assert!(scheduler.begin_compression());
        let token = CancelToken::new();
        scheduler.register_token(JobId(1), token);
        let active = scheduler.begin_cancel();
        assert!(active[0].1.is_cancelled());
        assert!(scheduler.is_locked());
        scheduler.finish_cancel();
        assert!(!scheduler.is_locked());
    }

    #[test]
    fn compression_parallelism_matches_frontend_cap() {
        assert_eq!(compression_concurrency(0, 16), 0);
        assert_eq!(compression_concurrency(1, 16), 1);
        assert_eq!(compression_concurrency(8, 1), 1);
        assert_eq!(compression_concurrency(8, 4), 2);
        assert_eq!(compression_concurrency(8, 16), 2);
        assert_eq!(compression_concurrency(8, 0), 2);
    }
}
