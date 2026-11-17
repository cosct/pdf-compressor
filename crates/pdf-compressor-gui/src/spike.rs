//! Phase-0 spike view: exercises the four capabilities the port depends on.
//!
//! 1. Self-drawn title bar with window controls (`TitleBar`).
//! 2. Files dropped from the file manager (`ExternalPaths`).
//! 3. The native multi-select file picker (`prompt_for_paths`).
//! 4. A blocking engine call on a background thread with progress streamed
//!    back to the view and a caller-owned cancel flag.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::prelude::*;
use gpui::{App, Context, Entity, ExternalPaths, PathPromptOptions, SharedString, Window, div, px};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::progress::Progress;
use gpui_component::{ActiveTheme, Disableable, TitleBar, h_flex, v_flex};
use gpui_kit::TestSupportExt;
use pdf_core::analyze_pdf_with_progress;
use pdf_core::models::{AnalysisResponse, ProgressUpdate};

/// Keep only `.pdf` paths (case-insensitive extension), like the Tauri
/// shell's `argv_pdf_paths` and the upload panel's drop filter. The native
/// picker has no extension filter, so this runs on its result too.
pub fn pdf_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
        })
        .collect()
}

enum Outcome {
    Idle,
    Running,
    Done(Box<AnalysisResponse>),
    Cancelled,
    Failed(String),
}

pub struct SpikeView {
    files: Vec<PathBuf>,
    outcome: Outcome,
    phase: SharedString,
    percent: f32,
    /// Owned by the view and created before the task is spawned, so a cancel
    /// can never arrive ahead of the task's registration.
    cancel: Option<Arc<AtomicBool>>,
}

impl SpikeView {
    pub fn build(_window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            files: Vec::new(),
            outcome: Outcome::Idle,
            phase: "".into(),
            percent: 0.,
            cancel: None,
        })
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<'_, Self>) {
        for path in pdf_paths(paths) {
            if !self.files.contains(&path) {
                self.files.push(path);
            }
        }
        cx.notify();
    }

    fn browse(&mut self, cx: &mut Context<'_, Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            // Outer Err: the view was dropped. Inner Err: the picker could
            // not be opened (e.g. no xdg-desktop-portal on Linux).
            let Ok(result) = picked.await else { return };
            let _ = this.update(cx, |this, cx| match result {
                Ok(Some(paths)) => this.add_paths(paths, cx),
                Ok(None) => {}
                Err(error) => {
                    this.outcome = Outcome::Failed(format!("file picker: {error:#}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn analyze_first(&mut self, cx: &mut Context<'_, Self>) {
        let Some(path) = self.files.first().cloned() else {
            return;
        };
        if matches!(self.outcome, Outcome::Running) {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(Arc::clone(&cancel));
        self.outcome = Outcome::Running;
        self.phase = "queued".into();
        self.percent = 0.;
        cx.notify();

        let (tx, rx) = async_channel::unbounded::<ProgressUpdate>();

        // Progress pump: engine thread → view. The sender is dropped when the
        // engine call returns, which ends the loop.
        cx.spawn(async move |this, cx| {
            while let Ok(first) = rx.recv().await {
                let mut latest = first;
                while let Ok(next) = rx.try_recv() {
                    latest = next;
                }
                let alive = this
                    .update(cx, |this, cx| {
                        this.phase = latest.phase.clone().into();
                        this.percent = latest.percent;
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let path = path.to_string_lossy().into_owned();
                    analyze_pdf_with_progress(&path, None, None, &cancel, |update| {
                        let _ = tx.send_blocking(update);
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.cancel = None;
                this.outcome = match result {
                    Ok(response) => Outcome::Done(Box::new(response)),
                    Err(pdf_core::AppError::Cancelled(_)) => Outcome::Cancelled,
                    Err(error) => Outcome::Failed(error.to_string()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn cancel(&mut self, _cx: &mut Context<'_, Self>) {
        if let Some(flag) = &self.cancel {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn render_outcome(&self, cx: &Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme();
        let text: SharedString = match &self.outcome {
            Outcome::Idle => "Drop PDF files here or browse.".into(),
            Outcome::Running => format!("{} · {:.0}%", self.phase, self.percent).into(),
            Outcome::Done(response) => format!(
                "{} pages · {} images · kind {} · recommended {} · est. {:.1}%",
                response.page_count,
                response.image_object_count,
                response.document_kind,
                response.recommended_preset,
                response.estimated_savings_percent,
            )
            .into(),
            Outcome::Cancelled => "Cancelled.".into(),
            Outcome::Failed(message) => message.clone().into(),
        };
        let color = match &self.outcome {
            Outcome::Failed(_) => theme.danger,
            Outcome::Done(_) => theme.success,
            _ => theme.muted_foreground,
        };
        div().text_sm().text_color(color).child(text)
    }
}

impl Render for SpikeView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme();
        let running = matches!(self.outcome, Outcome::Running);
        let has_files = !self.files.is_empty();

        let file_rows = self.files.iter().enumerate().map(|(index, path)| {
            div()
                .id(("file", index))
                .px_3()
                .py_1p5()
                .rounded(px(8.))
                .bg(theme.secondary)
                .text_sm()
                .truncate()
                .child(SharedString::from(path.display().to_string()))
        });

        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(
                TitleBar::new().child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("PDF Compressor"),
                ),
            )
            .child(
                v_flex()
                    .id("drop-zone")
                    .test_support()
                    .flex_1()
                    .min_h(px(0.))
                    .m_4()
                    .p_5()
                    .gap_4()
                    .rounded(px(16.))
                    .border_1()
                    .border_color(theme.border)
                    .drag_over::<ExternalPaths>(|style, _, _, cx| {
                        style.border_color(cx.theme().primary)
                    })
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                        this.add_paths(paths.paths().to_vec(), cx);
                    }))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("browse")
                                    .label("Browse")
                                    .disabled(running)
                                    .on_click(cx.listener(|this, _, _, cx| this.browse(cx))),
                            )
                            .child(
                                Button::new("analyze")
                                    .primary()
                                    .label("Analyze first file")
                                    .disabled(running || !has_files)
                                    .on_click(cx.listener(|this, _, _, cx| this.analyze_first(cx))),
                            )
                            .when(running, |row| {
                                row.child(
                                    Button::new("cancel")
                                        .danger()
                                        .label("Cancel")
                                        .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                                )
                            }),
                    )
                    .child(Progress::new("progress").value(self.percent))
                    .child(self.render_outcome(cx))
                    .child(
                        v_flex()
                            .id("files")
                            .flex_1()
                            .min_h(px(0.))
                            .gap_1p5()
                            .overflow_y_scroll()
                            .children(file_rows),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{
        AnyWindowHandle, AssetSource, Bounds, ElementId, FileDropEvent, Modifiers, MouseButton,
        MouseDownEvent, MouseUpEvent, Pixels, PlatformInput, Point, TestAppContext, WindowBounds,
        WindowOptions, point, size,
    };
    use gpui_kit::base::test_support;

    use super::*;

    fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<SpikeView>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(0.), px(0.)),
                        size: size(px(1200.), px(860.)),
                    })),
                    ..TitleBar::window_options()
                },
                cx,
                SpikeView::build,
            )
            .expect("open test window")
        })
    }

    fn render_frame(window: &mut Window, cx: &mut App) {
        window.refresh();
        window.draw(cx).clear(cx);
    }

    /// Centre of an observed element in the last drawn frame; panics with the
    /// registered ids when the element is missing or not visible.
    fn center_of(window: &Window, id: &'static str) -> Point<Pixels> {
        let element = test_support::find(window, &[], &ElementId::from(id)).unwrap_or_else(|| {
            panic!(
                "no element {id:?}; registered: {}",
                test_support::registered_paths(window)
            )
        });
        assert!(element.visible(), "element {id:?} is not visible");
        element.bounds().center()
    }

    /// A left click through the window's real hit testing.
    fn click(window: &mut Window, id: &'static str, cx: &mut App) {
        render_frame(window, cx);
        let position = center_of(window, id);
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
            }),
            cx,
        );
        render_frame(window, cx);
    }

    fn fixture_pdf(dir: &tempfile::TempDir, name: &str) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, pdf_core::testutil::jpeg_page_pdf_bytes(600, 400, 90))
            .expect("write fixture");
        path
    }

    /// Regression: the first real launch showed a title bar without window
    /// controls because no asset source was registered. The icons the title
    /// bar needs must exist in the set `main` registers.
    #[test]
    fn title_bar_control_icons_are_in_the_registered_asset_set() {
        for name in ["minimize", "maximize", "restore", "close"] {
            let path = format!("icons/window-{name}.svg");
            let bytes = gpui_kit::assets::Assets
                .load(&path)
                .expect("asset lookup")
                .unwrap_or_else(|| panic!("{path} missing from the bundled assets"));
            assert!(!bytes.is_empty(), "{path} is empty");
        }
    }

    #[gpui::test]
    fn dropped_files_are_filtered_and_listed(cx: &mut TestAppContext) {
        let (window, view) = open(cx);
        cx.update_window(window, |_, window, cx| {
            render_frame(window, cx);
            let position = center_of(window, "drop-zone");
            let paths = ExternalPaths(
                vec![
                    PathBuf::from("/tmp/a.pdf"),
                    PathBuf::from("/tmp/notes.txt"),
                    PathBuf::from("/tmp/B.PDF"),
                ]
                .into(),
            );
            window.dispatch_event(
                PlatformInput::FileDrop(FileDropEvent::Entered { position, paths }),
                cx,
            );
            render_frame(window, cx);
            window.dispatch_event(
                PlatformInput::FileDrop(FileDropEvent::Submit { position }),
                cx,
            );
            render_frame(window, cx);
        })
        .unwrap();

        cx.update(|cx| {
            assert_eq!(
                view.read(cx).files,
                vec![PathBuf::from("/tmp/a.pdf"), PathBuf::from("/tmp/B.PDF")]
            );
        });
    }

    #[gpui::test]
    fn picker_selection_is_multi_select_and_filtered(cx: &mut TestAppContext) {
        let (window, view) = open(cx);
        cx.update_window(window, |_, window, cx| click(window, "browse", cx))
            .unwrap();
        assert!(cx.did_prompt_for_paths(), "Browse must open the picker");

        cx.simulate_path_prompt_response(|options| {
            assert!(options.files && options.multiple && !options.directories);
            Some(vec![
                PathBuf::from("/tmp/one.pdf"),
                PathBuf::from("/tmp/two.png"),
            ])
        });
        cx.run_until_parked();

        cx.update(|cx| {
            assert_eq!(view.read(cx).files, vec![PathBuf::from("/tmp/one.pdf")]);
        });
    }

    #[gpui::test]
    fn analysis_result_and_progress_reach_the_view(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let pdf = fixture_pdf(&dir, "sample.pdf");
        let (window, view) = open(cx);
        cx.update(|cx| view.update(cx, |view, cx| view.add_paths(vec![pdf], cx)));

        cx.update_window(window, |_, window, cx| click(window, "analyze", cx))
            .unwrap();
        // The click only schedules the work — nothing has been polled yet —
        // so the running state must already be visible on its own.
        cx.update(|cx| {
            let view = view.read(cx);
            assert!(matches!(view.outcome, Outcome::Running));
            assert!(view.cancel.is_some());
        });

        cx.run_until_parked();

        cx.update(|cx| {
            let view = view.read(cx);
            match &view.outcome {
                Outcome::Done(response) => assert_eq!(response.page_count, 1),
                Outcome::Failed(message) => panic!("analysis failed: {message}"),
                _ => panic!("analysis did not finish"),
            }
            assert!(view.cancel.is_none(), "cancel flag must be released");
            assert!(
                !view.phase.is_empty() && view.phase.as_ref() != "queued",
                "engine progress never reached the view (phase {:?})",
                view.phase
            );
        });
    }

    #[gpui::test]
    fn cancel_requested_before_the_engine_starts_is_honoured(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let pdf = fixture_pdf(&dir, "sample.pdf");
        let (_window, view) = open(cx);

        // Start and cancel inside one update: nothing has been polled yet, so
        // this is the "cancel arrives before the task is registered" race that
        // the Tauri shell's task registry could lose.
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.add_paths(vec![pdf], cx);
                view.analyze_first(cx);
                view.cancel(cx);
            });
        });
        cx.run_until_parked();

        cx.update(|cx| {
            let view = view.read(cx);
            assert!(
                matches!(view.outcome, Outcome::Cancelled),
                "expected a cancelled outcome"
            );
            assert!(view.cancel.is_none());
        });
    }

    #[test]
    fn pdf_paths_keeps_only_pdf_extensions_case_insensitively() {
        let kept = pdf_paths([
            PathBuf::from("/tmp/a.pdf"),
            PathBuf::from("/tmp/b.PDF"),
            PathBuf::from("/tmp/c.txt"),
            PathBuf::from("/tmp/no-extension"),
            PathBuf::from("/tmp/dir.pdf/inner.png"),
        ]);
        assert_eq!(
            kept,
            vec![PathBuf::from("/tmp/a.pdf"), PathBuf::from("/tmp/b.PDF")]
        );
    }
}
