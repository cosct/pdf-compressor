//! Tauri application entry point — plugin registration, splash window, and command handlers.
//! Tauri 应用入口 — 插件注册、启动画面窗口和命令处理器。
//!
//! The PDF engine itself lives in the `pdf-core` workspace crate; this shell
//! only wires it to Tauri IPC, dialogs, and window management.
//! PDF 引擎位于 pdf-core 工作区 crate；本壳层只负责 Tauri IPC、对话框与窗口管理。

mod commands;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// PDF paths handed to the app before the frontend registered its
/// `open-pdf` listener: cold-start argv, very-early second instances, and
/// macOS `Opened` events that fire during launch. Drained and emitted by
/// the `app_ready` command, which the frontend invokes only after its
/// listener is up.
pub(crate) struct OpenPdfDispatch {
    state: Mutex<DispatchState>,
}

#[derive(Default)]
struct DispatchState {
    pending: Vec<String>,
    frontend_ready: bool,
}

impl Default for OpenPdfDispatch {
    fn default() -> Self {
        Self {
            state: Mutex::new(DispatchState::default()),
        }
    }
}

impl OpenPdfDispatch {
    /// Queue `paths` for replay, or hand them back for direct emission once
    /// the frontend has signaled readiness. The ready check and the push
    /// share one lock with [`Self::mark_ready_and_drain`]: a dispatch racing
    /// readiness either queues (and is drained) or emits — it can never
    /// strand paths in an already-drained queue.
    fn stage(&self, paths: Vec<String>) -> Option<Vec<String>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.frontend_ready {
            Some(paths)
        } else {
            state.pending.extend(paths);
            None
        }
    }

    /// Flip readiness and drain everything queued so far under the same
    /// lock; every later dispatch emits directly.
    fn mark_ready_and_drain(&self) -> Vec<String> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.frontend_ready = true;
        std::mem::take(&mut state.pending)
    }
}

/// Queue `paths` for delivery, or emit them straight away once the frontend
/// has signaled readiness. Never drops a handed-over PDF.
pub(crate) fn dispatch_open_pdf_paths(app: &tauri::AppHandle, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let state = app.state::<OpenPdfDispatch>();
    if let Some(ready_paths) = state.inner().stage(paths) {
        let _ = app.emit("open-pdf", serde_json::json!({ "paths": ready_paths }));
    }
}

/// Cold-start argv (and the macOS `Opened` event) hand the app PDF paths the
/// same way the single-instance plugin does for a running instance.
fn argv_pdf_paths<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    args.into_iter()
        .map(Into::into)
        .skip(1)
        .filter(|arg| {
            PathBuf::from(arg)
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
        })
        .collect()
}

/// Specta command registry — regenerates `frontend/src/lib/bindings.ts` via
/// `cargo test export_bindings` so the frontend always matches the backend.
fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    use tauri_specta::{collect_commands, Builder};

    Builder::<tauri::Wry>::new().commands(collect_commands![
        commands::load_preset_user_config,
        commands::save_preset_user_config,
        commands::clear_preset_user_config,
        commands::load_quick_profile,
        commands::save_quick_profile,
        commands::open_path,
        commands::reveal_path_in_folder,
        commands::build_features,
        commands::analyze_pdf,
        commands::compress_pdf,
        commands::compress_scanned_pdf,
        commands::cancel_compression,
        commands::existing_paths,
        commands::app_ready
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();

    tauri::Builder::default()
        .manage(commands::CompressionTaskRegistry::default())
        .manage(commands::SessionOutputRegistry::default())
        .manage(OpenPdfDispatch::default())
        // Must be the first plugin: routes "Open with…" launches of an
        // already-running instance back into the main window.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let pdf_paths = argv_pdf_paths(argv);
            if pdf_paths.is_empty() {
                return;
            }

            if let Some(main_window) = app.get_webview_window("main") {
                let _ = main_window.show();
                let _ = main_window.set_focus();
            }
            // Queued (not dropped) when the frontend listener is not up yet.
            dispatch_open_pdf_paths(app, pdf_paths);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            // Cold-start "Open with…": the argv the OS handed us at launch.
            // Non-UTF-8 paths are skipped rather than fatal (args_os, not
            // args — the CLI learned this the hard way).
            let cold_start = std::env::args_os()
                .filter_map(|arg| arg.into_string().ok())
                .collect::<Vec<String>>();
            let cold_start_pdfs = argv_pdf_paths(cold_start);
            if !cold_start_pdfs.is_empty() {
                // The frontend is never ready this early — stage() queues.
                let _ = app
                    .state::<OpenPdfDispatch>()
                    .inner()
                    .stage(cold_start_pdfs);
            }

            if let Some(main_window) = app.get_webview_window("main") {
                if let Err(e) = main_window.hide() {
                    log::warn!("Failed to hide main window during splash: {e}");
                }
            }

            if let Err(e) = WebviewWindowBuilder::new(
                app,
                "splash",
                WebviewUrl::App(PathBuf::from("splash.html")),
            )
            .title("Loading")
            .inner_size(420.0, 320.0)
            .resizable(false)
            .decorations(false)
            .always_on_top(true)
            .center()
            .build()
            {
                log::warn!("Failed to create splash window: {e}");
            }

            // Frontend-failure backstop: if `app_ready` never arrives (a
            // broken bundle, a webview crash), surface the main window
            // anyway instead of stranding the user on the always-on-top
            // splash forever. A normal start closes the splash long before
            // this fires and the branch becomes a no-op.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(10));
                if let Some(splash) = handle.get_webview_window("splash") {
                    let _ = splash.close();
                    if let Some(main) = handle.get_webview_window("main") {
                        let _ = main.show();
                        let _ = main.set_focus();
                    }
                }
            });

            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // macOS "Open with…" after launch arrives as an Opened event,
            // not argv; route it through the same dispatch.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = event {
                let pdf_paths: Vec<String> = urls
                    .into_iter()
                    .filter_map(|url| url.to_file_path().ok())
                    .filter(|path| {
                        path.extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
                    })
                    .filter_map(|path| path.to_str().map(str::to_string))
                    .collect();
                dispatch_open_pdf_paths(app, pdf_paths);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}

#[cfg(test)]
mod dispatch_tests {
    use std::sync::Arc;
    use std::thread;

    use super::OpenPdfDispatch;

    #[test]
    fn paths_handed_over_before_readiness_replay_exactly_once() {
        let dispatch = OpenPdfDispatch::default();
        assert_eq!(
            dispatch.stage(vec!["a.pdf".to_string()]),
            None,
            "pre-readiness deliveries queue instead of emitting"
        );
        assert_eq!(
            dispatch.mark_ready_and_drain(),
            vec!["a.pdf".to_string()],
            "readiness drains everything queued so far"
        );
        assert!(
            dispatch.mark_ready_and_drain().is_empty(),
            "a second drain finds nothing — replay is exactly-once"
        );
    }

    #[test]
    fn paths_handed_over_after_readiness_emit_directly() {
        let dispatch = OpenPdfDispatch::default();
        dispatch.mark_ready_and_drain();
        assert_eq!(
            dispatch.stage(vec!["b.pdf".to_string()]),
            Some(vec!["b.pdf".to_string()])
        );
    }

    /// Conservation under the interleaving the old design could not survive
    /// (ready flag observed between the check and the push left paths
    /// stranded in a drained queue): every handed-over path is delivered
    /// exactly once, whatever the thread scheduling.
    #[test]
    fn concurrent_dispatch_and_readiness_never_lose_paths() {
        let dispatch = Arc::new(OpenPdfDispatch::default());
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let dispatch = Arc::clone(&dispatch);
                thread::spawn(move || {
                    if i % 4 == 3 {
                        dispatch.mark_ready_and_drain()
                    } else {
                        dispatch
                            .stage(vec![format!("doc-{i}.pdf")])
                            .unwrap_or_default()
                    }
                })
            })
            .collect();
        let mut delivered: Vec<String> = handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("dispatch thread"))
            .collect();
        delivered.extend(dispatch.mark_ready_and_drain());
        delivered.sort();

        let mut expected: Vec<String> = (0..8)
            .filter(|i| i % 4 != 3)
            .map(|i| format!("doc-{i}.pdf"))
            .collect();
        expected.sort();
        assert_eq!(delivered, expected);
    }
}

#[cfg(test)]
mod specta_export {
    use super::specta_builder;

    /// Regenerates the TypeScript IPC bindings consumed by the frontend.
    /// Run via `cargo test export_bindings` (also part of `cargo test`).
    #[test]
    fn export_bindings() {
        specta_builder()
            .export(
                specta_typescript::Typescript::default(),
                "../frontend/src/lib/bindings.ts",
            )
            .expect("failed to export typescript bindings");
    }
}
