//! Tauri application entry point — plugin registration, splash window, and command handlers.
//! Tauri 应用入口 — 插件注册、启动画面窗口和命令处理器。
//!
//! The PDF engine itself lives in the `pdf-core` workspace crate; this shell
//! only wires it to Tauri IPC, dialogs, and window management.
//! PDF 引擎位于 pdf-core 工作区 crate；本壳层只负责 Tauri IPC、对话框与窗口管理。

mod commands;

use std::path::PathBuf;

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

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
        // Must be the first plugin: routes "Open with…" launches of an
        // already-running instance back into the main window.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let pdf_paths: Vec<String> = argv
                .into_iter()
                .skip(1)
                .filter(|arg| {
                    std::path::Path::new(arg)
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
                })
                .collect();

            if pdf_paths.is_empty() {
                return;
            }

            if let Some(main_window) = app.get_webview_window("main") {
                let _ = main_window.show();
                let _ = main_window.set_focus();
                let _ = app.emit("open-pdf", serde_json::json!({ "paths": pdf_paths }));
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
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

            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
