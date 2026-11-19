use std::path::PathBuf;

use pdf_core::models::{AnalysisResponse, CompressionResponse, ProgressUpdate};

use super::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Selected,
    Analyzing,
    Ready,
    Compressing,
    Success,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressPhase {
    Queued,
    Analyzing,
    Compressing,
    Writing,
    Done,
    Error,
}

impl ProgressPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Analyzing => "analyzing",
            Self::Compressing => "compressing",
            Self::Writing => "writing",
            Self::Done => "done",
            Self::Error => "error",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "analyzing" => Self::Analyzing,
            "compressing" => Self::Compressing,
            "writing" => Self::Writing,
            "done" => Self::Done,
            "error" => Self::Error,
            _ => Self::Queued,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobProgress {
    pub phase: ProgressPhase,
    pub percent: f32,
}

impl Default for JobProgress {
    fn default() -> Self {
        Self {
            phase: ProgressPhase::Queued,
            percent: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: JobId,
    pub source_path: PathBuf,
    pub file_name: String,
    pub status: JobStatus,
    pub progress: JobProgress,
    pub analysis: Option<AnalysisResponse>,
    pub result: Option<CompressionResponse>,
    pub settings: Settings,
    pub recommended_settings: Settings,
    pub use_recommended_settings: bool,
    /// Restored queue settings are protected from the first analysis result.
    pub settings_restored: bool,
    /// Session-only; intentionally excluded from persistence.
    pub password: Option<String>,
    pub error: Option<pdf_core::AppErrorPayload>,
    /// Increased for each analysis/compression attempt; async results must
    /// carry the token they started with and are ignored when it no longer fits.
    pub run_token: u64,
}

impl Job {
    pub fn new(id: JobId, source_path: PathBuf, settings: Settings) -> Self {
        let file_name = super::format::file_name(&source_path);
        Self {
            id,
            source_path,
            file_name,
            status: JobStatus::Selected,
            progress: JobProgress::default(),
            analysis: None,
            result: None,
            recommended_settings: settings.clone(),
            settings,
            use_recommended_settings: true,
            settings_restored: false,
            password: None,
            error: None,
            run_token: 0,
        }
    }

    pub fn begin_analysis(&mut self) -> u64 {
        self.run_token = self.run_token.wrapping_add(1);
        self.status = JobStatus::Analyzing;
        self.progress = JobProgress {
            phase: ProgressPhase::Analyzing,
            percent: 0.0,
        };
        self.error = None;
        self.result = None;
        self.run_token
    }

    pub fn begin_compression(&mut self) -> u64 {
        self.run_token = self.run_token.wrapping_add(1);
        self.status = JobStatus::Compressing;
        self.progress = JobProgress {
            phase: ProgressPhase::Compressing,
            percent: 0.0,
        };
        self.error = None;
        self.result = None;
        self.run_token
    }

    pub fn apply_progress(
        &mut self,
        token: u64,
        source_path: &std::path::Path,
        update: ProgressUpdate,
    ) {
        if self.run_token != token || self.source_path != source_path {
            return;
        }
        let percent = if update.percent.is_finite() {
            update.percent.clamp(0.0, 100.0)
        } else {
            0.0
        };
        let phase = ProgressPhase::parse(&update.phase);
        if phase != ProgressPhase::Done {
            self.status = match phase {
                ProgressPhase::Analyzing => JobStatus::Analyzing,
                ProgressPhase::Compressing | ProgressPhase::Writing => JobStatus::Compressing,
                ProgressPhase::Error => JobStatus::Error,
                ProgressPhase::Queued | ProgressPhase::Done => JobStatus::Selected,
            };
        }
        self.progress = JobProgress { phase, percent };
    }

    pub fn finish_analysis(
        &mut self,
        token: u64,
        source_path: &std::path::Path,
        analysis: AnalysisResponse,
        recommended: Settings,
    ) -> bool {
        if self.run_token != token || self.source_path != source_path {
            return false;
        }
        self.analysis = Some(analysis);
        self.recommended_settings = recommended;
        if self.settings_restored {
            self.settings_restored = false;
            self.use_recommended_settings = self.settings.comparable_eq(&self.recommended_settings);
        } else {
            let output_dir = self.settings.output_dir.clone();
            let target_size = self.settings.target_file_size_mb;
            self.settings = self.recommended_settings.clone();
            self.settings.output_dir = output_dir;
            self.settings.target_file_size_mb = target_size;
            self.use_recommended_settings = true;
        }
        self.progress = JobProgress::default();
        self.status = JobStatus::Ready;
        self.error = None;
        true
    }

    pub fn fail(
        &mut self,
        token: u64,
        source_path: &std::path::Path,
        error: pdf_core::AppErrorPayload,
    ) -> bool {
        if self.run_token != token || self.source_path != source_path {
            return false;
        }
        self.error = Some(error);
        self.progress = JobProgress {
            phase: ProgressPhase::Error,
            percent: 0.0,
        };
        self.status = JobStatus::Error;
        true
    }

    pub fn finish_compression(
        &mut self,
        token: u64,
        source_path: &std::path::Path,
        result: CompressionResponse,
    ) -> bool {
        if self.run_token != token || self.source_path != source_path {
            return false;
        }
        self.result = Some(result);
        self.progress = JobProgress {
            phase: ProgressPhase::Done,
            percent: 100.0,
        };
        self.status = JobStatus::Success;
        self.error = None;
        true
    }

    pub fn cancel_settled(&mut self) {
        self.error = None;
        self.result = None;
        self.progress = JobProgress::default();
        self.status = if self.analysis.is_some() {
            JobStatus::Ready
        } else {
            JobStatus::Selected
        };
    }
}
