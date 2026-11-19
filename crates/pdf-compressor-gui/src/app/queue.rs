use std::{collections::HashSet, path::PathBuf};

use pdf_core::{
    AppError, AppErrorPayload,
    models::{AnalysisResponse, CompressionResponse, ProgressUpdate},
};

use super::{
    format,
    job::{Job, JobId, JobStatus},
    presets::Presets,
    settings::{Preset, Settings},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueMutation {
    Changed,
    Unchanged,
    Locked,
}

#[derive(Debug, Default)]
pub struct Queue {
    pub jobs: Vec<Job>,
    pub selected: Option<JobId>,
    pub draft_settings: Settings,
    pub presets: Presets,
    next_id: u64,
    pub compression_running: bool,
    pub cancellation_in_flight: bool,
}

impl Queue {
    pub fn new() -> Self {
        Self {
            draft_settings: Settings::default(),
            ..Self::default()
        }
    }

    fn allocate_id(&mut self) -> JobId {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        JobId(self.next_id)
    }

    pub fn find(&self, id: JobId) -> Option<&Job> {
        self.jobs.iter().find(|job| job.id == id)
    }

    pub fn find_mut(&mut self, id: JobId) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|job| job.id == id)
    }

    pub fn selected_job(&self) -> Option<&Job> {
        self.selected.and_then(|id| self.find(id))
    }

    pub fn select(&mut self, id: JobId) -> bool {
        if self.find(id).is_some() {
            self.selected = Some(id);
            true
        } else {
            false
        }
    }

    pub fn add_paths(
        &mut self,
        paths: impl IntoIterator<Item = PathBuf>,
        restored: &[(PathBuf, Settings)],
    ) -> (QueueMutation, Vec<JobId>) {
        if self.is_locked() {
            return (QueueMutation::Locked, Vec::new());
        }
        let mut existing: HashSet<PathBuf> = self
            .jobs
            .iter()
            .map(|job| job.source_path.clone())
            .collect();
        let mut added = Vec::new();
        for path in paths {
            let text = path.to_string_lossy().trim().to_owned();
            if text.is_empty() {
                continue;
            }
            let path = PathBuf::from(text);
            if !existing.insert(path.clone()) {
                continue;
            }
            let restored_settings = restored
                .iter()
                .find(|(restored_path, _)| restored_path == &path)
                .map(|(_, settings)| settings.clone());
            let settings = restored_settings
                .clone()
                .unwrap_or_else(|| self.draft_settings.clone())
                .normalize();
            let mut job = Job::new(self.allocate_id(), path.clone(), settings);
            if restored_settings.is_some() {
                job.settings_restored = true;
                job.use_recommended_settings = false;
            } else {
                job.use_recommended_settings = false;
            }
            let id = job.id;
            self.jobs.push(job);
            added.push(id);
            if self.selected.is_none() {
                self.selected = Some(id);
            }
        }
        let mutation = if added.is_empty() {
            QueueMutation::Unchanged
        } else {
            QueueMutation::Changed
        };
        (mutation, added)
    }

    pub fn remove(&mut self, id: JobId) -> QueueMutation {
        if self.is_locked() {
            return QueueMutation::Locked;
        }
        let Some(index) = self.jobs.iter().position(|job| job.id == id) else {
            return QueueMutation::Unchanged;
        };
        self.jobs.remove(index);
        if self.selected == Some(id) {
            self.selected = self
                .jobs
                .get(index)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|previous| self.jobs.get(previous))
                })
                .map(|job| job.id);
        }
        QueueMutation::Changed
    }

    pub fn is_locked(&self) -> bool {
        self.compression_running || self.cancellation_in_flight
    }

    pub fn analysis_targets(&self) -> Vec<JobId> {
        self.jobs
            .iter()
            .filter(|job| format::is_pdf(&job.source_path))
            .filter(|job| job.status == JobStatus::Selected)
            .map(|job| job.id)
            .collect()
    }

    pub fn pending_compression_targets(&self) -> Vec<JobId> {
        self.jobs
            .iter()
            .filter(|job| format::is_pdf(&job.source_path))
            .filter(|job| !matches!(job.status, JobStatus::Compressing | JobStatus::Success))
            .map(|job| job.id)
            .collect()
    }

    pub fn selected_compression_target(&self) -> Vec<JobId> {
        self.selected_job()
            .filter(|job| format::is_pdf(&job.source_path))
            .filter(|job| job.status != JobStatus::Compressing)
            .map(|job| vec![job.id])
            .unwrap_or_default()
    }

    pub fn update_selected_settings(&mut self, settings: Settings) -> bool {
        let settings = settings.normalize();
        let Some(id) = self.selected else {
            self.draft_settings = settings;
            return false;
        };
        let Some(job) = self.find_mut(id) else {
            self.draft_settings = settings;
            return false;
        };
        job.settings = settings;
        job.use_recommended_settings = job.settings.comparable_eq(&job.recommended_settings);
        true
    }

    pub fn apply_selected_settings_to_all(&mut self) -> bool {
        let Some(selected) = self
            .selected_job()
            .map(|job| job.settings.clone().normalize())
        else {
            return false;
        };
        let selected_id = self.selected;
        for job in &mut self.jobs {
            if Some(job.id) != selected_id && !job.source_path.as_os_str().is_empty() {
                job.settings = selected.clone();
                job.use_recommended_settings =
                    job.settings.comparable_eq(&job.recommended_settings);
            }
        }
        true
    }

    pub fn begin_analysis(
        &mut self,
        id: JobId,
    ) -> Option<(u64, PathBuf, Settings, Option<String>)> {
        let job = self.find_mut(id)?;
        if !format::is_pdf(&job.source_path) || job.status == JobStatus::Compressing {
            return None;
        }
        let token = job.begin_analysis();
        Some((
            token,
            job.source_path.clone(),
            job.settings.clone(),
            job.password.clone(),
        ))
    }

    pub fn apply_progress(
        &mut self,
        id: JobId,
        token: u64,
        path: &std::path::Path,
        update: ProgressUpdate,
    ) -> bool {
        let Some(job) = self.find_mut(id) else {
            return false;
        };
        if job.run_token != token || job.source_path != path {
            return false;
        }
        job.apply_progress(token, path, update);
        true
    }

    pub fn finish_analysis(
        &mut self,
        id: JobId,
        token: u64,
        path: &std::path::Path,
        analysis: AnalysisResponse,
    ) -> bool {
        let Some(existing_job) = self.find(id) else {
            return false;
        };
        if existing_job.run_token != token || existing_job.source_path != path {
            return false;
        }
        let output_dir = existing_job
            .settings
            .output_dir
            .clone()
            .or_else(|| self.draft_settings.output_dir.clone());
        let target_size = existing_job
            .settings
            .target_file_size_mb
            .or(self.draft_settings.target_file_size_mb);
        let preset = Preset::parse(&analysis.recommended_preset);
        let mut recommended = self.presets.settings_for(preset);
        recommended.reference_max_image_edge_px =
            (analysis.max_image_edge_px > 0).then_some(u32::from(analysis.max_image_edge_px));
        recommended.output_dir = output_dir;
        recommended.target_file_size_mb = target_size;
        let Some(job) = self.find_mut(id) else {
            return false;
        };
        job.finish_analysis(token, path, analysis, recommended)
    }

    pub fn fail(&mut self, id: JobId, token: u64, path: &std::path::Path, error: AppError) -> bool {
        self.find_mut(id).is_some_and(|job| {
            if matches!(error, AppError::Cancelled(_)) {
                if job.run_token != token || job.source_path != path {
                    return false;
                }
                job.cancel_settled();
                true
            } else {
                job.fail(token, path, AppErrorPayload::from(error))
            }
        })
    }

    pub fn begin_compression(
        &mut self,
        id: JobId,
    ) -> Option<(u64, PathBuf, Settings, Option<String>, bool)> {
        let job = self.find_mut(id)?;
        if !format::is_pdf(&job.source_path) {
            return None;
        }
        let scanned = job
            .analysis
            .as_ref()
            .is_some_and(AnalysisResponse::wants_scanned_pipeline);
        let token = job.begin_compression();
        Some((
            token,
            job.source_path.clone(),
            job.settings.clone(),
            job.password.clone(),
            scanned,
        ))
    }

    pub fn finish_compression(
        &mut self,
        id: JobId,
        token: u64,
        path: &std::path::Path,
        result: CompressionResponse,
    ) -> bool {
        self.find_mut(id)
            .is_some_and(|job| job.finish_compression(token, path, result))
    }

    pub fn cancel_running_jobs(&mut self) {
        for job in &mut self.jobs {
            if matches!(job.status, JobStatus::Analyzing | JobStatus::Compressing) {
                job.cancel_settled();
                // Invalidate late callbacks/results while the real engine call
                // drains. The scheduler holds the cancel flag until it settles.
                job.run_token = job.run_token.wrapping_add(1);
            }
        }
    }

    pub fn submit_password(&mut self, id: JobId, password: String) -> bool {
        if self.compression_running {
            return false;
        }
        let Some(job) = self.find_mut(id) else {
            return false;
        };
        job.password = if password.is_empty() {
            None
        } else {
            Some(password)
        };
        job.error = None;
        job.result = None;
        job.progress = Default::default();
        job.status = JobStatus::Selected;
        true
    }

    pub fn needs_password(&self, id: JobId) -> bool {
        self.find(id)
            .and_then(|job| job.error.as_ref())
            .is_some_and(|error| {
                matches!(
                    error.code.as_str(),
                    "error.passwordRequired" | "error.wrongPassword"
                )
            })
    }

    pub fn selected_needs_password(&self) -> bool {
        self.selected.is_some_and(|id| self.needs_password(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_trims_deduplicates_exact_paths_and_selects_first() {
        let mut queue = Queue::new();
        let (changed, ids) = queue.add_paths(
            [
                PathBuf::from(" /tmp/a.pdf "),
                PathBuf::from("/tmp/a.pdf"),
                PathBuf::from("/tmp/B.PDF"),
            ],
            &[],
        );
        assert_eq!(changed, QueueMutation::Changed);
        assert_eq!(ids.len(), 2);
        assert_eq!(queue.jobs.len(), 2);
        assert_eq!(queue.selected, Some(ids[0]));
    }

    #[test]
    fn additions_allow_non_pdf_but_analysis_only_targets_pdf() {
        let mut queue = Queue::new();
        let (_, ids) = queue.add_paths(
            [PathBuf::from("/tmp/a.txt"), PathBuf::from("/tmp/b.PDF")],
            &[],
        );
        assert_eq!(queue.analysis_targets(), vec![ids[1]]);
    }

    #[test]
    fn remove_selected_chooses_next_then_previous_and_refuses_when_locked() {
        let mut queue = Queue::new();
        let (_, ids) = queue.add_paths(["a.pdf", "b.pdf", "c.pdf"].map(PathBuf::from), &[]);
        queue.select(ids[1]);
        assert_eq!(queue.remove(ids[1]), QueueMutation::Changed);
        assert_eq!(queue.selected, Some(ids[2]));
        queue.compression_running = true;
        assert_eq!(queue.remove(ids[2]), QueueMutation::Locked);
        queue.compression_running = false;
        queue.select(ids[2]);
        assert_eq!(queue.remove(ids[2]), QueueMutation::Changed);
        assert_eq!(queue.selected, Some(ids[0]));
    }

    #[test]
    fn pending_and_selected_targets_match_queue_scope_rules() {
        let mut queue = Queue::new();
        let (_, ids) = queue.add_paths(["a.pdf", "b.pdf"].map(PathBuf::from), &[]);
        queue.find_mut(ids[1]).unwrap().status = JobStatus::Success;
        assert_eq!(queue.pending_compression_targets(), vec![ids[0]]);
        queue.select(ids[1]);
        assert_eq!(queue.selected_compression_target(), vec![ids[1]]);
    }

    #[test]
    fn restored_settings_are_seeded_before_analysis_and_kept_once() {
        let mut queue = Queue::new();
        let restored = Settings {
            image_quality: 44,
            ..Settings::default()
        };
        let (_, ids) = queue.add_paths(
            [PathBuf::from("/tmp/a.pdf")],
            &[(PathBuf::from("/tmp/a.pdf"), restored)],
        );
        let (token, path, _, _) = queue.begin_analysis(ids[0]).unwrap();
        let analysis = fake_analysis("maximum", 1600);
        assert!(queue.finish_analysis(ids[0], token, &path, analysis));
        let job = queue.find(ids[0]).unwrap();
        assert_eq!(job.settings.image_quality, 44);
        assert!(!job.settings_restored);
        assert_eq!(job.recommended_settings.image_quality, 46);
    }

    #[test]
    fn stale_results_and_progress_are_ignored() {
        let mut queue = Queue::new();
        let (_, ids) = queue.add_paths([PathBuf::from("/tmp/a.pdf")], &[]);
        let (token, path, _, _) = queue.begin_analysis(ids[0]).unwrap();
        let fake = fake_analysis("balanced", 1600);
        let next = queue.begin_analysis(ids[0]).unwrap().0;
        assert!(next > token);
        assert!(!queue.finish_analysis(ids[0], token, &path, fake));
        assert!(!queue.apply_progress(
            ids[0],
            token,
            &path,
            ProgressUpdate::new("analyzing", 40.0)
        ));
    }

    fn fake_analysis(preset: &str, edge: u16) -> AnalysisResponse {
        AnalysisResponse {
            file_size_bytes: 1000.0,
            page_count: 1,
            image_object_count: 1,
            document_kind: "scan-heavy".into(),
            scanned_confidence: 90.0,
            image_coverage: 90.0,
            estimated_savings_percent: 80.0,
            recommended_preset: preset.into(),
            max_image_edge_px: edge,
            recommended_max_image_size_px: edge,
            recommended_image_quality: 50,
            is_likely_scanned: true,
            notices: vec![],
        }
    }
}
