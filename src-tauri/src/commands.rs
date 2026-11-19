//! Tauri command surface — the IPC boundary between the Vue frontend and Rust backend.
//! Tauri 命令接口层 — Vue 前端与 Rust 后端之间的 IPC 边界。
//!
//! Responsibilities: preset config persistence (save/load/clear), PDF analysis
//! and compression dispatch, compression task cancellation, open/reveal via
//! system handler, and splash-to-main window transition.
//! 职责：预设配置持久化（保存/加载/清除）、PDF 分析与压缩调度、
//! 压缩任务取消、通过系统处理器打开/显示文件、启动画面到主窗口的切换。

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
};

use tauri::ipc::Channel;
use tauri::{Manager, State};

use pdf_core::{
    analyze_pdf_with_progress, build_features as engine_build_features,
    compress_pdf_to_target_size, compress_pdf_with_progress,
    models::{
        AnalysisResponse, BuildFeatures, CompressPdfRequest, CompressScannedPdfRequest,
        CompressionResponse, PresetUserConfigPayload, ProgressUpdate, QuickProfilePayload,
    },
    preset_config, AppError, AppErrorPayload, BilevelCodec, CompressionSettings,
    CompressionSettingsOverrides,
};

#[derive(Debug, Clone, Default)]
pub struct CompressionTaskRegistry {
    tasks: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl CompressionTaskRegistry {
    fn tasks(&self) -> Result<MutexGuard<'_, HashMap<String, Arc<AtomicBool>>>, AppError> {
        self.tasks
            .lock()
            .map_err(|_| AppError::Config("Compression task registry mutex poisoned.".to_string()))
    }

    fn register(&self, task_id: String) -> Result<Arc<AtomicBool>, AppError> {
        let mut tasks = self.tasks()?;
        // A duplicate id would silently corrupt cancellation: the overwrite
        // orphans the first task's cancel flag (it can never be cancelled),
        // and whichever task finishes first unregisters the other's entry.
        // The GUI always sends unique per-run ids; the path-fallback id makes
        // this reachable only for concurrent IPC calls on the same file —
        // surface the collision instead of racing.
        if tasks.contains_key(&task_id) {
            return Err(AppError::Config(format!(
                "A compression task is already registered for '{task_id}'."
            )));
        }
        let cancel_flag = Arc::new(AtomicBool::new(false));
        tasks.insert(task_id, Arc::clone(&cancel_flag));
        Ok(cancel_flag)
    }

    fn cancel(&self, task_id: &str) -> Result<(), AppError> {
        if let Some(cancel_flag) = self.tasks()?.get(task_id).cloned() {
            cancel_flag.store(true, Ordering::Relaxed);
        }

        Ok(())
    }

    fn unregister(&self, task_id: &str) -> Result<(), AppError> {
        self.tasks()?.remove(task_id);
        Ok(())
    }
}

/// Outputs produced by this session. `open_path`/`reveal_path_in_folder`
/// only accept paths in this registry, keeping the IPC surface from turning
/// into a generic "open arbitrary file with the OS" primitive.
#[derive(Debug, Clone, Default)]
pub struct SessionOutputRegistry {
    outputs: Arc<Mutex<HashSet<PathBuf>>>,
}

impl SessionOutputRegistry {
    fn canonical(path: &Path) -> PathBuf {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    pub fn register(&self, path: &str) {
        let canonical = Self::canonical(&PathBuf::from(path));
        if let Ok(mut outputs) = self.outputs.lock() {
            outputs.insert(canonical);
        }
    }

    fn contains(&self, path: &str) -> bool {
        let canonical = Self::canonical(&PathBuf::from(path));
        self.outputs
            .lock()
            .map(|outputs| outputs.contains(&canonical))
            .unwrap_or(false)
    }
}

fn open_path_with_system(path: &Path) -> Result<(), AppError> {
    let canonical = path
        .canonicalize()
        .map_err(|_| AppError::MissingInput(path.to_path_buf()))?;
    opener::open(&canonical)
        .map_err(|e| AppError::Opener(format!("Failed to open path with system handler: {e}")))
}

fn reveal_path_in_folder_with_system(path: &Path) -> Result<(), AppError> {
    let canonical = path
        .canonicalize()
        .map_err(|_| AppError::MissingInput(path.to_path_buf()))?;
    opener::reveal(&canonical)
        .map_err(|e| AppError::Opener(format!("Failed to reveal path in folder: {e}")))
}

#[tauri::command]
#[specta::specta]
pub fn load_preset_user_config() -> Result<PresetUserConfigPayload, AppErrorPayload> {
    preset_config::resolve_preset_config_path()
        .and_then(|path| preset_config::read_preset_config_at(&path))
        .map_err(AppErrorPayload::from)
}

#[tauri::command]
#[specta::specta]
pub async fn save_preset_user_config(
    config: PresetUserConfigPayload,
) -> Result<PresetUserConfigPayload, AppErrorPayload> {
    // The writer fsyncs (write_all + sync_all) — keep that off the main
    // thread; sync commands run on it in Tauri v2.
    tauri::async_runtime::spawn_blocking(move || {
        preset_config::resolve_preset_config_path()
            .and_then(|path| preset_config::write_preset_config_at(&path, &config))
            .map_err(AppErrorPayload::from)?;
        Ok(config)
    })
    .await
    .map_err(|join| {
        AppErrorPayload::from(AppError::Config(format!(
            "background config save failed: {join}"
        )))
    })?
}

#[tauri::command]
#[specta::specta]
pub fn clear_preset_user_config() -> Result<(), AppErrorPayload> {
    preset_config::resolve_preset_config_path()
        .and_then(|path| preset_config::clear_preset_config_at(&path))
        .map_err(AppErrorPayload::from)
}

/// Quick-mode (right-click) profile, persisted at the shared OS-config
/// location so the CLI's `quick` subcommand picks up GUI edits.
#[tauri::command]
#[specta::specta]
pub fn load_quick_profile() -> Result<QuickProfilePayload, AppErrorPayload> {
    let path =
        pdf_core::quick_profile::default_quick_profile_path().map_err(AppErrorPayload::from)?;
    load_quick_profile_at(&path).map_err(AppErrorPayload::from)
}

/// Path-parameterized core of [`load_quick_profile`] so tests can point it
/// at a temp directory (same discipline as the preset config commands).
fn load_quick_profile_at(path: &Path) -> Result<QuickProfilePayload, AppError> {
    pdf_core::quick_profile::read_quick_profile_at(path)
}

#[tauri::command]
#[specta::specta]
pub fn save_quick_profile(
    profile: QuickProfilePayload,
) -> Result<QuickProfilePayload, AppErrorPayload> {
    let path =
        pdf_core::quick_profile::default_quick_profile_path().map_err(AppErrorPayload::from)?;
    save_quick_profile_at(&path, profile).map_err(AppErrorPayload::from)
}

/// Path-parameterized core of [`save_quick_profile`]. Writes and echoes the
/// saved profile back so the GUI can adopt any load-time normalization.
fn save_quick_profile_at(
    path: &Path,
    profile: QuickProfilePayload,
) -> Result<QuickProfilePayload, AppError> {
    pdf_core::quick_profile::write_quick_profile_at(path, &profile)?;
    pdf_core::quick_profile::read_quick_profile_at(path)
}

#[tauri::command]
#[specta::specta]
pub fn open_path(
    path: String,
    outputs: State<'_, SessionOutputRegistry>,
) -> Result<(), AppErrorPayload> {
    if !outputs.contains(&path) {
        return Err(AppErrorPayload::from(AppError::Config(
            "Only files produced during this session can be opened.".to_string(),
        )));
    }

    open_path_with_system(&PathBuf::from(path)).map_err(AppErrorPayload::from)
}

#[tauri::command]
#[specta::specta]
pub async fn reveal_path_in_folder(
    path: String,
    outputs: State<'_, SessionOutputRegistry>,
) -> Result<(), AppErrorPayload> {
    if !outputs.contains(&path) {
        return Err(AppErrorPayload::from(AppError::Config(
            "Only files produced during this session can be revealed.".to_string(),
        )));
    }

    // opener::reveal blocks on D-Bus (Linux) or waits on a child process
    // (macOS) — a sync command would freeze the UI thread for its duration.
    tauri::async_runtime::spawn_blocking(move || {
        reveal_path_in_folder_with_system(&PathBuf::from(path)).map_err(AppErrorPayload::from)
    })
    .await
    .map_err(|join| {
        AppErrorPayload::from(AppError::Config(format!(
            "background reveal failed: {join}"
        )))
    })?
}

/// Which optional engine components this build carries (0.9.0 honesty
/// pass): lets the UI state plainly what a feature-off source build will
/// not do instead of silently accepting toggles it cannot honor. Release
/// artifacts ship all features; only source builds differ.
#[tauri::command]
#[specta::specta]
pub fn build_features() -> Result<BuildFeatures, AppErrorPayload> {
    Ok(engine_build_features())
}

#[tauri::command]
#[specta::specta]
pub async fn analyze_pdf(
    task_id: String,
    path: Option<String>,
    input_path: Option<String>,
    password: Option<String>,
    settings: Option<pdf_core::models::CompressionSettingsPayload>,
    on_progress: Channel<ProgressUpdate>,
    registry: State<'_, CompressionTaskRegistry>,
) -> Result<AnalysisResponse, AppErrorPayload> {
    let requested_path = input_path.or(path).ok_or_else(|| {
        AppErrorPayload::from(AppError::PdfBuild(
            "No input path was provided to analyze_pdf.".to_string(),
        ))
    })?;

    // Settings context: the estimate follows the caller's actual toggles
    // (CMYK conversion, size cap, preset) instead of the default posture.
    let analysis_settings = settings.map(|payload| {
        CompressionSettings::from_sources(Some(payload), CompressionSettingsOverrides::default())
    });

    // 0.11.0: analysis registers with the same task registry as compression,
    // so the cancel button works while a large document is still being
    // scanned (the engine checks at entry and inside the page/object loops).
    let cancel_flag = registry
        .register(task_id.clone())
        .map_err(AppErrorPayload::from)?;
    let join_result = tauri::async_runtime::spawn_blocking(move || {
        analyze_pdf_with_progress(
            &requested_path,
            password.as_deref(),
            analysis_settings.as_ref(),
            &cancel_flag,
            |update| {
                let _ = on_progress.send(update);
            },
        )
    })
    .await;
    registry
        .unregister(&task_id)
        .map_err(AppErrorPayload::from)?;

    join_result
        .map_err(|error| {
            AppErrorPayload::from(AppError::PdfBuild(format!(
                "Failed to join analyze task: {error}"
            )))
        })?
        .map_err(AppErrorPayload::from)
}

/// Bundle of resolved request fields consumed by [`run_compression`].
struct CompressionJob {
    requested_path: String,
    task_id: String,
    password: Option<String>,
    target_size_bytes: Option<u32>,
    settings: CompressionSettings,
    task_label: &'static str,
}

/// Shared compression pipeline: register the cancel flag, dispatch to the
/// target-size or standard engine entry point, unregister, and record the
/// produced output. `task_label` names the command in join errors.
async fn run_compression(
    job: CompressionJob,
    on_progress: Channel<ProgressUpdate>,
    registry: &CompressionTaskRegistry,
    outputs: &SessionOutputRegistry,
) -> Result<CompressionResponse, AppErrorPayload> {
    let CompressionJob {
        requested_path,
        task_id,
        password,
        target_size_bytes,
        settings,
        task_label,
    } = job;
    let cancel_flag = registry
        .register(task_id.clone())
        .map_err(AppErrorPayload::from)?;

    let join_result = tauri::async_runtime::spawn_blocking(move || {
        let mut progress = |update: ProgressUpdate| {
            let _ = on_progress.send(update);
        };
        match target_size_bytes {
            Some(target) => compress_pdf_to_target_size(
                &requested_path,
                password.as_deref(),
                u64::from(target),
                settings,
                cancel_flag,
                &mut progress,
            ),
            None => compress_pdf_with_progress(
                &requested_path,
                password.as_deref(),
                settings,
                cancel_flag,
                progress,
            ),
        }
    })
    .await;
    registry
        .unregister(&task_id)
        .map_err(AppErrorPayload::from)?;

    let response = join_result
        .map_err(|error| {
            AppErrorPayload::from(AppError::PdfBuild(format!(
                "Failed to join {task_label} task: {error}"
            )))
        })?
        .map_err(AppErrorPayload::from)?;
    outputs.register(&response.output_path);
    Ok(response)
}

#[tauri::command]
#[specta::specta]
pub async fn compress_pdf(
    request: CompressPdfRequest,
    on_progress: Channel<ProgressUpdate>,
    registry: State<'_, CompressionTaskRegistry>,
    outputs: State<'_, SessionOutputRegistry>,
) -> Result<CompressionResponse, AppErrorPayload> {
    let requested_path = request.input_path.or(request.path).ok_or_else(|| {
        AppErrorPayload::from(AppError::PdfBuild(
            "No input path was provided to compress_pdf.".to_string(),
        ))
    })?;
    let task_id = request.task_id.unwrap_or_else(|| requested_path.clone());
    let target_size_bytes = request.target_size_bytes.filter(|bytes| *bytes > 0);

    let merged_settings = CompressionSettings::from_sources(
        request.settings,
        CompressionSettingsOverrides {
            preset: request.preset,
            image_quality: request.image_quality,
            max_image_size_px: request.max_image_size_px,
            max_image_size_percent: request.max_image_size_percent,
            optimize_images: request.optimize_images,
            compress_streams: request.compress_streams,
            strip_metadata: request.strip_metadata.or(request.remove_metadata),
            // No override-level flags here — grayscale / bilevel codec can
            // still arrive via the settings payload.
            grayscale: None,
            bilevel_codec: None,
            subset_fonts: None,
            cmyk_conversion: None,
            output_dir: request.output_dir,
        },
    );

    run_compression(
        CompressionJob {
            requested_path,
            task_id,
            password: request.password.filter(|value| !value.is_empty()),
            target_size_bytes,
            settings: merged_settings,
            task_label: "compress",
        },
        on_progress,
        registry.inner(),
        outputs.inner(),
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn compress_scanned_pdf(
    request: CompressScannedPdfRequest,
    on_progress: Channel<ProgressUpdate>,
    registry: State<'_, CompressionTaskRegistry>,
    outputs: State<'_, SessionOutputRegistry>,
) -> Result<CompressionResponse, AppErrorPayload> {
    let requested_path = request.input_path.or(request.path).ok_or_else(|| {
        AppErrorPayload::from(AppError::PdfBuild(
            "No input path was provided to compress_scanned_pdf.".to_string(),
        ))
    })?;
    let task_id = request.task_id.unwrap_or_else(|| requested_path.clone());
    let target_size_bytes = request.target_size_bytes.filter(|bytes| *bytes > 0);

    let merged_settings = CompressionSettings::from_sources(
        request.settings,
        CompressionSettingsOverrides {
            preset: request.preset,
            image_quality: request.image_quality,
            max_image_size_px: request.max_image_size_px,
            max_image_size_percent: request.max_image_size_percent,
            // Forced on by `scanned_pipeline()` below.
            optimize_images: None,
            compress_streams: None,
            grayscale: request.grayscale,
            bilevel_codec: request
                .bilevel_codec
                .as_deref()
                .map(|value| BilevelCodec::from_optional_str(Some(value))),
            subset_fonts: None,
            cmyk_conversion: None,
            strip_metadata: request.strip_metadata.or(request.remove_metadata),
            output_dir: request.output_dir,
        }
        .scanned_pipeline(),
    );

    run_compression(
        CompressionJob {
            requested_path,
            task_id,
            password: request.password.filter(|value| !value.is_empty()),
            target_size_bytes,
            settings: merged_settings,
            task_label: "scanned compress",
        },
        on_progress,
        registry.inner(),
        outputs.inner(),
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub fn cancel_compression(
    task_id: String,
    registry: State<'_, CompressionTaskRegistry>,
) -> Result<(), AppErrorPayload> {
    registry.cancel(&task_id).map_err(AppErrorPayload::from)?;
    Ok(())
}

/// Filter `paths` down to the ones that exist on disk. Used by the frontend
/// to pre-check a restored session queue instead of waiting for each missing
/// file to fail analysis.
#[tauri::command]
#[specta::specta]
pub async fn existing_paths(paths: Vec<String>) -> Result<Vec<String>, AppErrorPayload> {
    // Path::exists can block on network mounts (UNC paths may prompt for
    // SMB credentials on Windows) — keep the probes off the main thread.
    tauri::async_runtime::spawn_blocking(move || {
        paths
            .into_iter()
            .filter(|path| !path.trim().is_empty() && Path::new(path).exists())
            .collect::<Vec<String>>()
    })
    .await
    .map_err(|join| {
        AppErrorPayload::from(AppError::Config(format!("existence probe failed: {join}")))
    })
}

#[tauri::command]
#[specta::specta]
pub fn app_ready(app: tauri::AppHandle) -> Result<(), AppErrorPayload> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }

    if let Some(splash) = app.get_webview_window("splash") {
        let _ = splash.close();
    }

    // The frontend signals readiness only after its `open-pdf` listener is
    // registered: flip readiness and replay any PDFs handed over during
    // launch (cold-start argv, early second instances, macOS Opened events)
    // under one lock — dispatches racing this call queue instead of being
    // stranded in a drained queue — then every later delivery emits directly.
    let dispatch = app.state::<crate::OpenPdfDispatch>();
    let replayed = dispatch.inner().mark_ready_and_drain();
    if !replayed.is_empty() {
        use tauri::Emitter as _;
        let _ = app.emit("open-pdf", serde_json::json!({ "paths": replayed }));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn task_registry_register_cancel_and_unregister() {
        let registry = CompressionTaskRegistry::default();

        let flag = registry.register("task-1".to_string()).expect("register");
        assert!(!flag.load(Ordering::Relaxed));

        registry.cancel("task-1").expect("cancel");
        assert!(flag.load(Ordering::Relaxed));

        registry.unregister("task-1").expect("unregister");
        // Cancelling an unknown task is a no-op, not an error.
        registry.cancel("task-1").expect("cancel unknown");
    }

    #[test]
    fn task_registry_rejects_duplicate_registration() {
        let registry = CompressionTaskRegistry::default();
        registry.register("task-1".to_string()).expect("register");

        // Overwriting would orphan the first task's cancel flag and let the
        // first finisher unregister the other's entry — the collision is an
        // error so the caller can back off.
        let duplicate = registry.register("task-1".to_string());
        assert!(duplicate.is_err());

        // After unregister the id is free again.
        registry.unregister("task-1").expect("unregister");
        registry
            .register("task-1".to_string())
            .expect("re-register");
    }

    #[test]
    fn output_registry_tracks_session_outputs() {
        let outputs = SessionOutputRegistry::default();
        outputs.register("/definitely/not/a/real/path.pdf");

        assert!(outputs.contains("/definitely/not/a/real/path.pdf"));
        assert!(!outputs.contains("/definitely/not/a/real/other.pdf"));
    }

    #[test]
    fn output_registry_canonicalizes_real_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("out.pdf");
        fs::write(&file, b"%PDF-1.5").expect("write");

        let outputs = SessionOutputRegistry::default();
        outputs.register(&file.to_string_lossy());

        // A non-canonical spelling of the same file still matches.
        let mut lexical = file.into_os_string().into_string().unwrap();
        if !lexical.starts_with('/') {
            lexical = format!("/{lexical}");
        }
        assert!(outputs.contains(&lexical));
    }

    #[test]
    fn quick_profile_commands_roundtrip_through_their_cores() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("quick-profile.json");

        // Missing file reads as the (migrated, current-version) default.
        let missing = load_quick_profile_at(&path).expect("load missing");
        assert_eq!(missing.version, 3);
        assert_eq!(missing.cmyk_conversion, None);

        // A save echoes back the persisted (load-normalized) profile, not
        // the caller's object — the GUI adopts the same view the CLI's
        // next read will see.
        let saved = save_quick_profile_at(
            &path,
            QuickProfilePayload {
                preset: Some("maximum".to_string()),
                grayscale: Some(true),
                ..Default::default()
            },
        )
        .expect("save");
        assert_eq!(saved.preset.as_deref(), Some("maximum"));
        assert_eq!(saved.grayscale, Some(true));
        assert_eq!(saved.version, 3);

        let reloaded = load_quick_profile_at(&path).expect("reload");
        assert_eq!(reloaded.preset.as_deref(), Some("maximum"));
    }

    #[test]
    fn existing_paths_command_filters_missing_and_blank_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.pdf");
        fs::write(&file, b"%PDF-1.5").expect("write");

        let kept = tauri::async_runtime::block_on(existing_paths(vec![
            file.to_string_lossy().into_owned(),
            "/definitely/missing.pdf".to_string(),
            "   ".to_string(),
        ]))
        .expect("existing_paths");

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0], file.to_string_lossy());
    }
}
