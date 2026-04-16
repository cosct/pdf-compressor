//! Tauri application entry point — plugin registration, splash window, and command handlers.
//! Tauri 应用入口 — 插件注册、启动画面窗口和命令处理器。

mod commands;
mod error;
mod models;
mod pdf;

use std::path::PathBuf;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(commands::CompressionTaskRegistry::default())
        .plugin(tauri_plugin_dialog::init())
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
        .invoke_handler(tauri::generate_handler![
            commands::load_preset_user_config,
            commands::save_preset_user_config,
            commands::clear_preset_user_config,
            commands::open_path,
            commands::reveal_path_in_folder,
            commands::analyze_pdf,
            commands::compress_pdf,
            commands::compress_scanned_pdf,
            commands::cancel_compression,
            commands::app_ready
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
