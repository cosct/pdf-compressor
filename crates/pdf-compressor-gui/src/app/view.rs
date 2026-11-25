use std::{collections::VecDeque, path::PathBuf, time::Duration};

use async_channel::Receiver;
use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Entity, ExternalPaths, PathPromptOptions, Window, WindowBounds,
    WindowOptions, div, px, size,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputContentType, InputState};
use gpui_component::progress::Progress;
use gpui_component::{ActiveTheme, Disableable, TitleBar, h_flex, v_flex};
use pdf_core::models::ProgressUpdate;

use super::{
    engine,
    job::{JobId, JobStatus},
    locale::{self, Text},
    persistence,
    prefs::{LocalePreference, Preferences, ThemePreference},
    presets::Presets,
    queue::{Queue, QueueMutation},
    scheduler::{CancelToken, Scheduler, compression_concurrency},
    settings::Preset,
};

pub struct AppView {
    queue: Queue,
    scheduler: Scheduler,
    analysis_running: bool,
    compression_targets: VecDeque<JobId>,
    compression_workers: usize,
    compression_limit: usize,
    preferences: Preferences,
    output_dir_input: Entity<InputState>,
    target_size_input: Entity<InputState>,
    password_input: Entity<InputState>,
    toasts: super::toasts::Toasts,
    state_path: Option<PathBuf>,
    persist_generation: u64,
    last_error: Option<String>,
}

impl AppView {
    pub fn build(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let state_path = persistence::default_queue_state_path().ok();
        Self::build_with_state_path(window, cx, state_path)
    }

    fn build_with_state_path(
        window: &mut Window,
        cx: &mut App,
        state_path: Option<PathBuf>,
    ) -> Entity<Self> {
        let mut queue = Queue::new();
        if let Ok(presets) = Presets::load_default() {
            queue.presets = presets;
            queue.draft_settings = queue.presets.settings_for(Preset::Balanced);
        }
        let output_dir_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Output directory"));
        let target_size_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Target size (MB)"));
        let password_input = cx.new(|cx| InputState::new(window, cx).placeholder("PDF password"));
        let mut restored_entries = Vec::new();
        let mut preferences = Preferences::default();
        let mut restore_error = None;
        if let Some(path) = state_path.as_deref() {
            match persistence::load_gui_state_at(path) {
                Ok(state) => {
                    preferences = state.preferences;
                    restored_entries = state.entries;
                }
                Err(error) => restore_error = Some(format!("queue restore: {error}")),
            }
        }
        let view = Self {
            queue,
            scheduler: Scheduler::default(),
            analysis_running: false,
            compression_targets: VecDeque::new(),
            compression_workers: 0,
            compression_limit: 0,
            preferences,
            output_dir_input,
            target_size_input,
            password_input,
            toasts: super::toasts::Toasts::default(),
            state_path,
            persist_generation: 0,
            last_error: restore_error,
        };
        let entries = std::mem::take(&mut restored_entries);
        apply_preferences(&view.preferences, Some(window), cx);
        let window_handle = window.window_handle();
        let entity = cx.new(|_| view);
        let weak = entity.downgrade();
        cx.spawn(async move |cx| {
            let existing = cx
                .background_executor()
                .spawn(async move {
                    let original_count = entries.len();
                    let existing = entries
                        .into_iter()
                        .filter(|entry| PathBuf::from(&entry.source_path).exists())
                        .collect::<Vec<_>>();
                    (original_count, existing)
                })
                .await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                weak.update(cx, |view, cx| {
                    let (original_count, entries) = existing;
                    if original_count > entries.len() && !entries.is_empty() {
                        view.toasts.push(super::notices::Notice {
                            code: "queue:restoredMissing".into(),
                            tone: super::notices::NoticeTone::Warning,
                            values: Default::default(),
                            fallback: format!(
                                "Skipped {} missing PDF file(s) while restoring the queue.",
                                original_count - entries.len()
                            ),
                        });
                    }
                    let (_, ids) = persistence::restore_queue(&mut view.queue, &entries);
                    for id in ids {
                        view.scheduler.queue_analysis(id);
                    }
                    view.sync_selected_inputs(window, cx);
                    view.start_next_analysis(cx);
                    cx.notify();
                })
            });
        })
        .detach();
        entity
    }

    fn sync_selected_inputs(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let settings = self
            .queue
            .selected_job()
            .map(|job| job.settings.clone())
            .unwrap_or_else(|| self.queue.draft_settings.clone());
        let output_dir = settings.output_dir.unwrap_or_default();
        let target_size = settings
            .target_file_size_mb
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.output_dir_input
            .update(cx, |input, cx| input.set_value(output_dir, window, cx));
        self.target_size_input
            .update(cx, |input, cx| input.set_value(target_size, window, cx));
    }

    fn set_theme(&mut self, preference: ThemePreference, cx: &mut Context<'_, Self>) {
        self.preferences.theme = preference;
        apply_preferences(&self.preferences, None, cx);
        self.save_queue(cx);
        cx.notify();
    }

    fn set_locale(&mut self, preference: LocalePreference, cx: &mut Context<'_, Self>) {
        self.preferences.locale = preference;
        apply_preferences(&self.preferences, None, cx);
        self.save_queue(cx);
        cx.notify();
    }

    fn save_queue(&mut self, cx: &mut Context<'_, Self>) {
        let Some(path) = self.state_path.clone() else {
            return;
        };
        self.persist_generation = self.persist_generation.wrapping_add(1);
        let generation = self.persist_generation;
        let jobs = self.queue.jobs.clone();
        let preferences = self.preferences.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(800))
                .await;
            let current = this
                .update(cx, |this, _| this.persist_generation == generation)
                .unwrap_or(false);
            if !current {
                return;
            }
            let result = cx
                .background_executor()
                .spawn(async move { persistence::save_gui_state_at(&path, &jobs, &preferences) })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |this, cx| {
                    this.last_error = Some(error.to_string());
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn receive_external_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<'_, Self>) {
        cx.activate(true);
        if !paths.is_empty() {
            self.add_paths(paths, cx);
        }
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<'_, Self>) {
        if self.queue.is_locked() {
            self.last_error = Some("The queue is locked while compression is running.".into());
            self.toasts
                .push(super::notices::Notice::from(pdf_core::AppError::Config(
                    "The queue is locked while compression is running.".into(),
                )));
            cx.notify();
            return;
        }
        let paths = paths
            .into_iter()
            .filter(|path| super::format::is_pdf(path))
            .collect::<Vec<_>>();
        let (mutation, ids) = self.queue.add_paths(paths, &[]);
        if mutation == QueueMutation::Locked {
            self.last_error = Some("The queue is locked while compression is running.".into());
            cx.notify();
            return;
        }
        for id in ids {
            self.scheduler.queue_analysis(id);
        }
        self.save_queue(cx);
        cx.notify();
        self.start_next_analysis(cx);
    }

    fn browse(&mut self, cx: &mut Context<'_, Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = picked.await else { return };
            let _ = this.update(cx, |this, cx| match result {
                Ok(Some(paths)) => this.add_paths(paths, cx),
                Ok(None) => {}
                Err(error) => {
                    this.last_error = Some(format!("file picker: {error:#}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn start_next_analysis(&mut self, cx: &mut Context<'_, Self>) {
        if self.analysis_running
            || self.scheduler.cancellation_in_flight()
            || self.scheduler.compression_running()
        {
            return;
        }
        let Some(id) = self.scheduler.pop_analysis() else {
            self.start_pending_compression(cx);
            return;
        };
        let Some((token, path, settings, password)) = self.queue.begin_analysis(id) else {
            self.scheduler.finish_analysis(id);
            self.start_next_analysis(cx);
            return;
        };
        self.analysis_running = true;
        let cancel = CancelToken::new();
        self.scheduler.register_token(id, cancel.clone());
        let (tx, rx) = async_channel::unbounded::<ProgressUpdate>();
        self.start_progress_pump(id, token, path.clone(), rx, cx);

        cx.spawn(async move |this, cx| {
            let path_for_worker = path.clone();
            let cancel_flag = cancel.flag();
            let result = cx
                .background_executor()
                .spawn(async move {
                    engine::analyze(
                        &path_for_worker,
                        password.as_deref(),
                        &settings,
                        &cancel_flag,
                        |update| {
                            let _ = tx.send_blocking(update);
                        },
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.scheduler.unregister_token(id, &cancel);
                this.scheduler.finish_analysis(id);
                this.analysis_running = false;
                if this.scheduler.cancellation_in_flight() {
                    this.finish_cancel_if_idle();
                } else {
                    match result {
                        Ok(analysis) => {
                            for note in analysis.notices.clone() {
                                if matches!(note.level.as_str(), "warning" | "danger") {
                                    this.toasts.push(note.into());
                                }
                            }
                            this.queue.finish_analysis(id, token, &path, analysis);
                        }
                        Err(error) => {
                            this.compression_targets.retain(|target| *target != id);
                            if !matches!(error, pdf_core::AppError::Cancelled(_)) {
                                this.toasts.push(super::notices::Notice::from_error(&error));
                            }
                            this.queue.fail(id, token, &path, error);
                        }
                    }
                }
                this.save_queue(cx);
                cx.notify();
                this.start_next_analysis(cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn start_progress_pump(
        &self,
        id: JobId,
        token: u64,
        path: PathBuf,
        rx: Receiver<ProgressUpdate>,
        cx: &mut Context<'_, Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Ok(first) = rx.recv().await {
                let mut latest = first;
                while let Ok(next) = rx.try_recv() {
                    latest = next;
                }
                let alive = this
                    .update(cx, |this, cx| {
                        if !this.scheduler.cancellation_in_flight() {
                            if let Some(note) = latest.message.clone()
                                && matches!(note.level.as_str(), "warning" | "danger")
                            {
                                this.toasts.push(note.into());
                            }
                            this.queue.apply_progress(id, token, &path, latest);
                        }
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    fn choose_preset(&mut self, preset: Preset, window: &mut Window, cx: &mut Context<'_, Self>) {
        let mut settings = self.queue.presets.settings_for(preset);
        if let Some(current) = self.queue.selected_job() {
            settings.output_dir = current.settings.output_dir.clone();
            settings.target_file_size_mb = None;
        } else {
            settings.output_dir = self.queue.draft_settings.output_dir.clone();
            settings.target_file_size_mb = None;
        }
        settings.preset = preset;
        self.queue.update_selected_settings(settings);
        self.sync_selected_inputs(window, cx);
        self.save_queue(cx);
        cx.notify();
    }

    fn apply_settings_to_all(&mut self, cx: &mut Context<'_, Self>) {
        if self.queue.apply_selected_settings_to_all() {
            self.save_queue(cx);
            cx.notify();
        }
    }

    fn adjust_number_setting(
        &mut self,
        field: &'static str,
        amount: i16,
        cx: &mut Context<'_, Self>,
    ) {
        let mut settings = self
            .queue
            .selected_job()
            .map(|job| job.settings.clone())
            .unwrap_or_else(|| self.queue.draft_settings.clone());
        match field {
            "quality" => {
                settings.image_quality =
                    (i16::from(settings.image_quality) + amount).clamp(1, 100) as u8;
                settings.preset = Preset::Custom;
            }
            "size-percent" => {
                settings.max_image_size_percent = (i32::from(settings.max_image_size_percent)
                    + i32::from(amount))
                .clamp(5, 100) as u16;
                settings.preset = Preset::Custom;
            }
            _ => return,
        }
        self.queue.update_selected_settings(settings);
        self.save_queue(cx);
        cx.notify();
    }

    fn apply_target_size(&mut self, cx: &mut Context<'_, Self>) {
        let value = self.target_size_input.read(cx).value().to_string();
        let target = value.trim().parse::<f64>().ok();
        let mut settings = self
            .queue
            .selected_job()
            .map(|job| job.settings.clone())
            .unwrap_or_else(|| self.queue.draft_settings.clone());
        settings.target_file_size_mb = target;
        self.queue.update_selected_settings(settings);
        self.save_queue(cx);
        cx.notify();
    }

    fn apply_output_dir(&mut self, cx: &mut Context<'_, Self>) {
        let value = self.output_dir_input.read(cx).value().to_string();
        let mut settings = self
            .queue
            .selected_job()
            .map(|job| job.settings.clone())
            .unwrap_or_else(|| self.queue.draft_settings.clone());
        settings.output_dir = Some(value);
        self.queue.update_selected_settings(settings.clone());
        if let Some(id) = self.queue.selected
            && let Some(job) = self.queue.find_mut(id)
        {
            job.recommended_settings.output_dir = settings.output_dir;
        }
        self.save_queue(cx);
        cx.notify();
    }

    fn submit_password(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(id) = self.queue.selected else {
            return;
        };
        let password = self.password_input.read(cx).value().to_string();
        if !self.queue.submit_password(id, password) {
            return;
        }
        self.password_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.scheduler.queue_analysis(id);
        self.start_next_analysis(cx);
        cx.notify();
    }

    fn browse_output_dir(&mut self, cx: &mut Context<'_, Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = picked.await else { return };
            let _ = this.update(cx, |this, cx| match result {
                Ok(Some(mut paths)) => {
                    if let Some(path) = paths.pop() {
                        let mut settings = this
                            .queue
                            .selected_job()
                            .map(|job| job.settings.clone())
                            .unwrap_or_else(|| this.queue.draft_settings.clone());
                        settings.output_dir = Some(path.to_string_lossy().into_owned());
                        this.queue.update_selected_settings(settings.clone());
                        if let Some(id) = this.queue.selected
                            && let Some(job) = this.queue.find_mut(id)
                        {
                            job.recommended_settings.output_dir = settings.output_dir;
                        }
                        this.save_queue(cx);
                        cx.notify();
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    this.last_error = Some(format!("directory picker: {error:#}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn select_job(&mut self, id: JobId, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.queue.select(id) {
            self.sync_selected_inputs(window, cx);
            cx.notify();
        }
    }

    fn dismiss_toast(&mut self, id: u64, cx: &mut Context<'_, Self>) {
        if self.toasts.dismiss(id) {
            cx.notify();
        }
    }

    fn set_bool_setting(&mut self, name: &'static str, value: bool, cx: &mut Context<'_, Self>) {
        let mut settings = self
            .queue
            .selected_job()
            .map(|job| job.settings.clone())
            .unwrap_or_else(|| self.queue.draft_settings.clone());
        match name {
            "optimize-images" => settings.optimize_images = value,
            "compress-streams" => settings.compress_streams = value,
            "strip-metadata" => settings.strip_metadata = value,
            "grayscale" => settings.grayscale = value,
            "subset-fonts" => settings.subset_fonts = value,
            "cmyk-conversion" => settings.cmyk_conversion = value,
            _ => return,
        }
        self.queue.update_selected_settings(settings);
        self.save_queue(cx);
        cx.notify();
    }

    fn toggle_bool_setting(&mut self, name: &'static str, cx: &mut Context<'_, Self>) {
        let current = self
            .queue
            .selected_job()
            .map(|job| &job.settings)
            .unwrap_or(&self.queue.draft_settings);
        let value = match name {
            "optimize-images" => !current.optimize_images,
            "compress-streams" => !current.compress_streams,
            "strip-metadata" => !current.strip_metadata,
            "grayscale" => !current.grayscale,
            "subset-fonts" => !current.subset_fonts,
            "cmyk-conversion" => !current.cmyk_conversion,
            _ => return,
        };
        self.set_bool_setting(name, value, cx);
    }

    fn remove_job(&mut self, id: JobId, cx: &mut Context<'_, Self>) {
        if self.queue.remove(id) == QueueMutation::Changed {
            self.save_queue(cx);
            cx.notify();
        }
    }

    fn compress_selected(&mut self, cx: &mut Context<'_, Self>) {
        self.compression_targets = self.queue.selected_compression_target().into();
        self.start_pending_compression(cx);
    }

    fn compress_all(&mut self, cx: &mut Context<'_, Self>) {
        self.compression_targets = self.queue.pending_compression_targets().into();
        self.start_pending_compression(cx);
    }

    fn start_pending_compression(&mut self, cx: &mut Context<'_, Self>) {
        if self.scheduler.cancellation_in_flight() || self.analysis_running {
            return;
        }
        if !self.scheduler.compression_running() {
            if self.queue.is_locked() || self.compression_targets.is_empty() {
                return;
            }
            // Drain all serialized analysis work before starting workers.
            // Jobs without prior analysis are queued here, matching the web
            // shell's per-worker analyze-on-demand behavior at the boundary.
            let needs_analysis: Vec<JobId> = self
                .compression_targets
                .iter()
                .copied()
                .filter(|id| {
                    self.queue
                        .find(*id)
                        .is_some_and(|job| job.analysis.is_none())
                })
                .collect();
            for id in needs_analysis {
                self.scheduler.queue_analysis(id);
            }
            if self.scheduler.has_pending_analysis() {
                self.start_next_analysis(cx);
                return;
            }
            if !self.scheduler.begin_compression() {
                return;
            }
            self.queue.compression_running = true;
            self.compression_limit = compression_concurrency(
                self.compression_targets.len(),
                std::thread::available_parallelism().map_or(4, usize::from),
            );
        }
        while self.compression_workers < self.compression_limit {
            let Some(id) = self.compression_targets.pop_front() else {
                break;
            };
            self.queue.select(id);
            let Some((token, path, settings, password, scanned)) = self.queue.begin_compression(id)
            else {
                continue;
            };
            let cancel = CancelToken::new();
            self.scheduler.register_token(id, cancel.clone());
            self.compression_workers += 1;
            let (tx, rx) = async_channel::unbounded::<ProgressUpdate>();
            self.start_progress_pump(id, token, path.clone(), rx, cx);
            cx.spawn(async move |this, cx| {
                let worker_path = path.clone();
                let cancel_flag = cancel.flag();
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        engine::compress(
                            &worker_path,
                            password.as_deref(),
                            &settings,
                            scanned,
                            cancel_flag,
                            |update| {
                                let _ = tx.send_blocking(update);
                            },
                        )
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.scheduler.unregister_token(id, &cancel);
                    this.compression_workers = this.compression_workers.saturating_sub(1);
                    if this.scheduler.cancellation_in_flight() {
                        this.finish_cancel_if_idle();
                    } else {
                        match result {
                            Ok(result) => {
                                let notes = result.notices.clone();
                                this.queue.finish_compression(id, token, &path, result);
                                for note in notes {
                                    if matches!(note.level.as_str(), "warning" | "danger") {
                                        this.toasts.push(note.into());
                                    }
                                }
                                if let Some(job) = this.queue.find(id)
                                    && job.result.as_ref().is_some_and(|result| !result.output_was_smaller)
                                {
                                    this.toasts.push(super::notices::Notice::from(pdf_core::AppError::Config("The original is already smaller; no output was written.".into())));
                                }
                            }
                            Err(error) => {
                                let notice = super::notices::Notice::from_error(&error);
                                this.toasts.push(notice);
                                this.queue.fail(id, token, &path, error);
                            }
                        }
                        if scanned {
                            this.toasts.push(super::notices::Notice {
                                code: "scan:pipeline".into(),
                                tone: super::notices::NoticeTone::Neutral,
                                values: Default::default(),
                                fallback: "Scan-heavy document: image and stream optimization are enabled.".into(),
                            });
                        }
                        this.start_pending_compression(cx);
                    }
                    this.save_queue(cx);
                    cx.notify();
                });
            })
            .detach();
        }
        if self.compression_workers == 0 && self.compression_targets.is_empty() {
            self.scheduler.finish_compression();
            self.queue.compression_running = false;
            self.compression_limit = 0;
        }
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<'_, Self>) {
        if !self.analysis_running && !self.scheduler.compression_running() {
            return;
        }
        self.scheduler.begin_cancel();
        self.compression_targets.clear();
        self.queue.cancellation_in_flight = true;
        self.queue.cancel_running_jobs();
        cx.notify();
        self.finish_cancel_if_idle();
    }

    fn finish_cancel_if_idle(&mut self) {
        if self.scheduler.cancellation_in_flight()
            && !self.analysis_running
            && self.compression_workers == 0
        {
            self.scheduler.finish_cancel();
            self.queue.cancellation_in_flight = false;
            self.queue.compression_running = false;
            self.compression_limit = 0;
        }
    }

    fn render_job(&self, id: JobId, cx: &Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme();
        let Some(job) = self.queue.find(id) else {
            return div().into_any_element();
        };
        let selected = self.queue.selected == Some(id);
        let file_name = job.file_name.clone();
        let status = format!("{:?}", job.status);
        let progress = format!(
            "{} · {:.0}%",
            job.progress.phase.as_str(),
            job.progress.percent
        );
        let detail = job
            .error
            .as_ref()
            .map(|error| error.fallback.clone())
            .unwrap_or(progress);
        h_flex()
            .id(("job", id.0))
            .w_full()
            .justify_between()
            .items_center()
            .gap_3()
            .p_3()
            .rounded(px(8.))
            .bg(if selected {
                theme.accent
            } else {
                theme.secondary
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_job(id, window, cx);
            }))
            .child(
                v_flex().flex_1().child(file_name).child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(detail),
                ),
            )
            .child(div().text_xs().child(status))
            .child(
                Button::new(("remove", id.0))
                    .label(locale::text(Text::Remove))
                    .disabled(self.queue.is_locked())
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_job(id, cx))),
            )
            .into_any_element()
    }

    fn render_toasts(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme();
        let rows = self.toasts.items().iter().map(|toast| {
            let id = toast.id;
            let tone = match toast.notice.tone {
                super::notices::NoticeTone::Warning => theme.warning,
                super::notices::NoticeTone::Success => theme.success,
                super::notices::NoticeTone::Danger => theme.danger,
                super::notices::NoticeTone::Neutral => theme.muted_foreground,
            };
            h_flex()
                .id(("toast", id))
                .w_full()
                .justify_between()
                .items_center()
                .gap_2()
                .p_2()
                .rounded(px(8.))
                .bg(theme.secondary)
                .child(
                    div()
                        .text_sm()
                        .text_color(tone)
                        .child(toast.notice.fallback.clone()),
                )
                .child(
                    Button::new(("dismiss-toast", id))
                        .label(locale::text(Text::Dismiss))
                        .on_click(cx.listener(move |this, _, _, cx| this.dismiss_toast(id, cx))),
                )
        });
        v_flex().w_full().gap_1().children(rows)
    }

    fn render_status(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme();
        if let Some(error) = &self.last_error {
            return div()
                .text_sm()
                .text_color(theme.danger)
                .child(error.clone())
                .into_any_element();
        }
        let selected = self.queue.selected_job();
        let text = selected.map_or_else(
            || "Add PDF files to begin.".to_owned(),
            |job| match job.status {
                JobStatus::Success => job.result.as_ref().map_or_else(
                    || "Compression finished.".into(),
                    |result| {
                        if result.output_path.is_empty() {
                            "The original is already smaller; no output was written.".into()
                        } else {
                            format!(
                                "Saved {:.1}% to {}",
                                result.savings_percent, result.output_path
                            )
                        }
                    },
                ),
                _ => job.analysis.as_ref().map_or_else(
                    || "Ready to analyze.".into(),
                    |analysis| {
                        format!(
                            "{} pages · {} images · {} · recommended {}",
                            analysis.page_count,
                            analysis.image_object_count,
                            analysis.document_kind,
                            analysis.recommended_preset
                        )
                    },
                ),
            },
        );
        div()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(text)
            .into_any_element()
    }
}

fn apply_preferences(preferences: &Preferences, window: Option<&mut Window>, cx: &mut App) {
    use gpui_component::theme::{Theme, ThemeMode};

    match preferences.theme {
        ThemePreference::System => Theme::sync_system_appearance(window, cx),
        ThemePreference::Light => Theme::change(ThemeMode::Light, window, cx),
        ThemePreference::Dark => Theme::change(ThemeMode::Dark, window, cx),
    }
    locale::set_locale(preferences.locale);
}

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme();
        let locked = self.queue.is_locked() || self.scheduler.cancellation_in_flight();
        let running = self.analysis_running || self.scheduler.compression_running();
        let ids: Vec<JobId> = self.queue.jobs.iter().map(|job| job.id).collect();
        let rows: Vec<_> = ids.into_iter().map(|id| self.render_job(id, cx)).collect();
        let has_selected = self.queue.selected_job().is_some();
        let many_jobs = self.queue.jobs.len() > 1;
        let needs_password = self.queue.selected_needs_password();
        let progress_value = self
            .queue
            .selected_job()
            .map_or(0.0, |job| job.progress.percent);
        let selected_settings = self
            .queue
            .selected_job()
            .map(|job| job.settings.clone())
            .unwrap_or_else(|| self.queue.draft_settings.clone());
        let text = |id: Text| locale::text(id);
        let state = |value: bool| text(if value { Text::On } else { Text::Off });
        let setting_label = |id: Text, value: bool| text(id).replace("%{state}", &state(value));
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(
                TitleBar::new().child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(text(Text::Title)),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .m_4()
                    .p_5()
                    .gap_3()
                    .rounded(px(16.))
                    .border_1()
                    .border_color(theme.border)
                    .drag_over::<ExternalPaths>(|style, _, _, cx| {
                        style.border_color(cx.theme().primary)
                    })
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                        this.add_paths(paths.paths().to_vec(), cx)
                    }))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("browse")
                                    .label(text(Text::AddPdfs))
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, _, cx| this.browse(cx))),
                            )
                            .child(
                                Button::new("compress-selected")
                                    .primary()
                                    .label(text(Text::CompressSelected))
                                    .disabled(
                                        locked
                                            || self.queue.selected_compression_target().is_empty(),
                                    )
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.compress_selected(cx)),
                                    ),
                            )
                            .child(
                                Button::new("compress-all")
                                    .label(text(Text::CompressPending))
                                    .disabled(
                                        locked
                                            || self.queue.pending_compression_targets().is_empty(),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| this.compress_all(cx))),
                            )
                            .child(
                                Button::new("cancel")
                                    .danger()
                                    .label(text(Text::Cancel))
                                    .disabled(!running)
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("preset-conservative")
                                    .label(text(Text::Conservative))
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_preset(Preset::Conservative, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("preset-balanced")
                                    .label(text(Text::Balanced))
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_preset(Preset::Balanced, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("preset-maximum")
                                    .label(text(Text::Maximum))
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_preset(Preset::Maximum, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("preset-custom")
                                    .label(text(Text::Custom))
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_preset(Preset::Custom, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("apply-all-settings")
                                    .label(text(Text::ApplyToAll))
                                    .disabled(locked || !has_selected || !many_jobs)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.apply_settings_to_all(cx)
                                        }),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("theme-system")
                                    .label(text(Text::SystemTheme))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.set_theme(ThemePreference::System, cx)
                                    })),
                            )
                            .child(
                                Button::new("theme-light")
                                    .label(text(Text::Light))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.set_theme(ThemePreference::Light, cx)
                                    })),
                            )
                            .child(Button::new("theme-dark").label(text(Text::Dark)).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.set_theme(ThemePreference::Dark, cx)
                                }),
                            ))
                            .child(
                                Button::new("locale-system")
                                    .label(text(Text::SystemLanguage))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.set_locale(LocalePreference::System, cx)
                                    })),
                            )
                            .child(
                                Button::new("locale-en")
                                    .label(text(Text::English))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.set_locale(LocalePreference::English, cx)
                                    })),
                            )
                            .child(
                                Button::new("locale-zh-cn")
                                    .label(text(Text::Chinese))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.set_locale(LocalePreference::ChineseSimplified, cx)
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Input::new(&self.output_dir_input)
                                    .id("output-dir")
                                    .w(px(300.)),
                            )
                            .child(
                                Button::new("browse-output-dir")
                                    .label(text(Text::ChooseFolder))
                                    .disabled(locked)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.browse_output_dir(cx)),
                                    ),
                            )
                            .child(
                                Input::new(&self.target_size_input)
                                    .id("target-size-mb")
                                    .w(px(120.)),
                            )
                            .child(
                                Button::new("apply-output-dir")
                                    .label(text(Text::ApplyDirectory))
                                    .disabled(locked)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.apply_output_dir(cx)),
                                    ),
                            )
                            .child(
                                Button::new("apply-target-size")
                                    .label(text(Text::ApplyTarget))
                                    .disabled(locked)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.apply_target_size(cx)),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("quality-down")
                                    .label("−")
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.adjust_number_setting("quality", -1, cx)
                                    })),
                            )
                            .child(div().text_sm().child(
                                text(Text::Quality).replace(
                                    "%{value}",
                                    &selected_settings.image_quality.to_string(),
                                ),
                            ))
                            .child(
                                Button::new("quality-up")
                                    .label("+")
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.adjust_number_setting("quality", 1, cx)
                                    })),
                            )
                            .child(
                                Button::new("size-percent-down")
                                    .label("− size")
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.adjust_number_setting("size-percent", -1, cx)
                                    })),
                            )
                            .child(div().text_sm().child(text(Text::MaxEdge).replace(
                                "%{value}",
                                &selected_settings.max_image_size_percent.to_string(),
                            )))
                            .child(
                                Button::new("size-percent-up")
                                    .label("+ size")
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.adjust_number_setting("size-percent", 1, cx)
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("toggle-optimize-images")
                                    .label(setting_label(
                                        Text::Images,
                                        selected_settings.optimize_images,
                                    ))
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_bool_setting("optimize-images", cx)
                                    })),
                            )
                            .child(
                                Button::new("toggle-compress-streams")
                                    .label(setting_label(
                                        Text::Streams,
                                        selected_settings.compress_streams,
                                    ))
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_bool_setting("compress-streams", cx)
                                    })),
                            )
                            .child(
                                Button::new("toggle-strip-metadata")
                                    .label(setting_label(
                                        Text::StripMetadata,
                                        selected_settings.strip_metadata,
                                    ))
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_bool_setting("strip-metadata", cx)
                                    })),
                            )
                            .child(
                                Button::new("toggle-grayscale")
                                    .label(setting_label(
                                        Text::Grayscale,
                                        selected_settings.grayscale,
                                    ))
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_bool_setting("grayscale", cx)
                                    })),
                            )
                            .child(
                                Button::new("toggle-subset-fonts")
                                    .label(setting_label(
                                        Text::SubsetFonts,
                                        selected_settings.subset_fonts,
                                    ))
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_bool_setting("subset-fonts", cx)
                                    })),
                            )
                            .child(
                                Button::new("toggle-cmyk")
                                    .label(setting_label(
                                        Text::Cmyk,
                                        selected_settings.cmyk_conversion,
                                    ))
                                    .disabled(locked)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_bool_setting("cmyk-conversion", cx)
                                    })),
                            ),
                    )
                    .child(Progress::new("progress").value(progress_value))
                    .when(needs_password, |root| {
                        root.child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Input::new(&self.password_input)
                                        .id("password")
                                        .content_type(InputContentType::Password)
                                        .w(px(240.)),
                                )
                                .child(
                                    Button::new("submit-password")
                                        .primary()
                                        .label(text(Text::RetryAnalysis))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.submit_password(window, cx)
                                        })),
                                ),
                        )
                    })
                    .child(self.render_status(cx))
                    .child(self.render_toasts(cx))
                    .child(
                        v_flex()
                            .id("queue")
                            .flex_1()
                            .min_h(px(0.))
                            .gap_2()
                            .overflow_y_scroll()
                            .children(rows),
                    ),
            )
    }
}

pub fn main_window_options(cx: &mut App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(1200.), px(860.)), cx);
    WindowOptions {
        app_id: Some("pdf-compressor".into()),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(1200.), px(800.))),
        window_decorations: Some(gpui::WindowDecorations::Client),
        ..TitleBar::window_options()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, point};

    fn open(cx: &mut TestAppContext) -> Entity<AppView> {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let bounds = Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(1200.), px(860.)),
            };
            let (_, view) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..TitleBar::window_options()
                },
                cx,
                |window, cx| AppView::build_with_state_path(window, cx, None),
            )
            .expect("open headless app window");
            view
        })
    }

    fn wait_until(
        cx: &mut TestAppContext,
        view: &Entity<AppView>,
        mut ready: impl FnMut(&AppView) -> bool,
    ) {
        for _ in 0..1_000 {
            cx.run_until_parked();
            if cx.update(|cx| ready(view.read(cx))) {
                return;
            }
            cx.executor().advance_clock(Duration::from_millis(10));
        }
        panic!("timed out waiting for app state");
    }

    fn fixture(dir: &tempfile::TempDir, name: &str) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, pdf_core::testutil::jpeg_page_pdf_bytes(600, 400, 88))
            .expect("write PDF fixture");
        path
    }

    #[gpui::test]
    fn app_state_serializes_analysis_then_compresses_pending_jobs(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = fixture(&dir, "first.pdf");
        let second = fixture(&dir, "second.pdf");
        let view = open(cx);
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                let (_, ids) = view.queue.add_paths([first, second], &[]);
                for id in ids {
                    view.scheduler.queue_analysis(id);
                }
                view.start_next_analysis(cx);
            });
        });

        wait_until(cx, &view, |view| {
            view.queue
                .jobs
                .iter()
                .all(|job| job.status == JobStatus::Ready)
        });

        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.compression_targets = view.queue.pending_compression_targets().into();
                view.start_pending_compression(cx);
            });
        });
        wait_until(cx, &view, |view| {
            view.queue
                .jobs
                .iter()
                .all(|job| job.status == JobStatus::Success)
                && !view.scheduler.compression_running()
        });

        cx.update(|cx| {
            let view = view.read(cx);
            assert_eq!(view.queue.jobs.len(), 2);
            assert!(view.queue.jobs.iter().all(|job| job.result.is_some()));
        });
    }

    #[gpui::test]
    fn cancel_before_analysis_worker_starts_is_honoured(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = fixture(&dir, "cancel.pdf");
        let view = open(cx);
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                let (_, ids) = view.queue.add_paths([path], &[]);
                view.scheduler.queue_analysis(ids[0]);
                view.start_next_analysis(cx);
                view.cancel(cx);
            });
        });

        wait_until(cx, &view, |view| {
            !view.scheduler.cancellation_in_flight()
                && view.queue.jobs[0].status == JobStatus::Selected
        });
    }
}
