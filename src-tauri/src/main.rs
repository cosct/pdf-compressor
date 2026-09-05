//! Desktop shell entry point — everything real lives in `app_lib::run()`
//! (plugin registration, windows, command handlers; see `lib.rs`).
//! 桌面壳入口 — 实际逻辑都在 `app_lib::run()`（见 lib.rs）。

// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    app_lib::run();
}
