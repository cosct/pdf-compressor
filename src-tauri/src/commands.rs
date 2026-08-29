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
    fs::{self, File},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
};

use tauri::ipc::Channel;
use tauri::{Manager, State};

use pdf_core::{
    analyze_pdf_with_progress, compress_pdf_to_target_size, compress_pdf_with_progress,
    models::{
        AnalysisResponse, CompressPdfRequest, CompressScannedPdfRequest, CompressionResponse,
        PresetUserConfigPayload, ProgressUpdate, QuickProfilePayload,
    },
    AppError, AppErrorPayload, BilevelCodec, CompressionSettings, CompressionSettingsOverrides,
};

/// Fallback config directory when the install directory is not writable.
fn app_config_dir_fallback() -> Result<PathBuf, AppError> {
    let base = dirs_next::config_dir().ok_or_else(|| {
        AppError::Config("Unable to determine OS application config directory.".to_string())
    })?;
    let dir = base.join("pdf-compressor");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

const PRESET_CONFIG_FILE_NAME: &str = "preset-user-config.json";

/// Try the OS config directory first (predictable, multi-user safe, works on
/// read-only install locations), falling back to the install directory
/// (portable mode) only when the config directory is unavailable. A legacy
/// install-dir config is migrated once.
fn resolve_preset_config_path() -> Result<PathBuf, AppError> {
    if let Ok(config_dir) = app_config_dir_fallback() {
        let candidate = config_dir.join(PRESET_CONFIG_FILE_NAME);

        if !candidate.exists() {
            if let Ok(executable_path) = std::env::current_exe() {
                if let Some(install_dir) = executable_path.parent() {
                    let legacy = install_dir.join(PRESET_CONFIG_FILE_NAME);
                    if legacy.exists() {
                        if let Ok(contents) = fs::read(&legacy) {
                            let _ = fs::write(&candidate, contents);
                        }
                    }
                }
            }
        }

        return Ok(candidate);
    }

    // Portable fallback: install directory when the OS config dir failed.
    if let Some(install_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        let candidate = install_dir.join(PRESET_CONFIG_FILE_NAME);
        if candidate.exists() || File::create(install_dir.join(".write-probe")).is_ok() {
            let _ = fs::remove_file(install_dir.join(".write-probe"));
            return Ok(candidate);
        }
    }

    Err(AppError::Config(
        "Unable to determine a writable location for the preset config.".to_string(),
    ))
}

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

fn read_preset_user_config() -> Result<PresetUserConfigPayload, AppError> {
    read_preset_config_at(&resolve_preset_config_path()?)
}

/// Path-parameterized core of [`read_preset_user_config`] so tests can point
/// it at a temp directory.
fn read_preset_config_at(config_path: &Path) -> Result<PresetUserConfigPayload, AppError> {
    let contents = match fs::read_to_string(config_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(PresetUserConfigPayload::default());
        }
        Err(error) => return Err(AppError::Io(error)),
    };

    if contents.trim().is_empty() {
        return Ok(PresetUserConfigPayload::default());
    }

    serde_json::from_str::<PresetUserConfigPayload>(&contents).map_err(|error| {
        AppError::Config(format!(
            "Failed to parse preset config at {}: {error}",
            config_path.display()
        ))
    })
}

fn write_preset_user_config(config: &PresetUserConfigPayload) -> Result<(), AppError> {
    write_preset_config_at(&resolve_preset_config_path()?, config)
}

/// Path-parameterized core of [`write_preset_user_config`]: serialize, write
/// to a temp file, sync, then atomically rename over the target.
fn write_preset_config_at(
    config_path: &Path,
    config: &PresetUserConfigPayload,
) -> Result<(), AppError> {
    let parent_dir = config_path.parent().ok_or_else(|| {
        AppError::Config(format!(
            "Unable to resolve parent directory for preset config: {}",
            config_path.display()
        ))
    })?;
    let temp_path = config_path.with_extension("tmp");
    let payload = serde_json::to_vec_pretty(config)
        .map_err(|error| AppError::Config(format!("Failed to serialize preset config: {error}")))?;

    fs::create_dir_all(parent_dir)?;

    {
        let mut temp_file = File::create(&temp_path)?;
        temp_file.write_all(&payload)?;
        temp_file.sync_all()?;
    }

    // Atomic on Unix; on Windows, try rename-over first (NTFS supports it),
    // falling back to remove-then-rename for older filesystems.
    if fs::rename(&temp_path, config_path).is_err() {
        let _ = fs::remove_file(config_path);
        fs::rename(&temp_path, config_path)?;
    }
    Ok(())
}

fn clear_preset_user_config_file() -> Result<(), AppError> {
    clear_preset_config_at(&resolve_preset_config_path()?)
}

/// Path-parameterized core of [`clear_preset_user_config_file`]; a missing
/// file is not an error.
fn clear_preset_config_at(config_path: &Path) -> Result<(), AppError> {
    match fs::remove_file(config_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::Io(error)),
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
    read_preset_user_config().map_err(AppErrorPayload::from)
}

#[tauri::command]
#[specta::specta]
pub fn save_preset_user_config(
    config: PresetUserConfigPayload,
) -> Result<PresetUserConfigPayload, AppErrorPayload> {
    write_preset_user_config(&config).map_err(AppErrorPayload::from)?;
    Ok(config)
}

#[tauri::command]
#[specta::specta]
pub fn clear_preset_user_config() -> Result<(), AppErrorPayload> {
    clear_preset_user_config_file().map_err(AppErrorPayload::from)
}

/// Quick-mode (right-click) profile, persisted at the shared OS-config
/// location so the CLI's `quick` subcommand picks up GUI edits.
#[tauri::command]
#[specta::specta]
pub fn load_quick_profile() -> Result<QuickProfilePayload, AppErrorPayload> {
    let path = pdf_core::quick_profile::default_quick_profile_path().map_err(AppErrorPayload::from)?;
    pdf_core::quick_profile::read_quick_profile_at(&path).map_err(AppErrorPayload::from)
}

#[tauri::command]
#[specta::specta]
pub fn save_quick_profile(
    profile: QuickProfilePayload,
) -> Result<QuickProfilePayload, AppErrorPayload> {
    let path = pdf_core::quick_profile::default_quick_profile_path().map_err(AppErrorPayload::from)?;
    pdf_core::quick_profile::write_quick_profile_at(&path, &profile)
        .map_err(AppErrorPayload::from)?;
    Ok(profile)
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
pub fn reveal_path_in_folder(
    path: String,
    outputs: State<'_, SessionOutputRegistry>,
) -> Result<(), AppErrorPayload> {
    if !outputs.contains(&path) {
        return Err(AppErrorPayload::from(AppError::Config(
            "Only files produced during this session can be revealed.".to_string(),
        )));
    }

    reveal_path_in_folder_with_system(&PathBuf::from(path)).map_err(AppErrorPayload::from)
}

#[tauri::command]
#[specta::specta]
pub async fn analyze_pdf(
    path: Option<String>,
    input_path: Option<String>,
    on_progress: Channel<ProgressUpdate>,
) -> Result<AnalysisResponse, AppErrorPayload> {
    let requested_path = input_path.or(path).ok_or_else(|| {
        AppErrorPayload::from(AppError::PdfBuild(
            "No input path was provided to analyze_pdf.".to_string(),
        ))
    })?;

    tauri::async_runtime::spawn_blocking(move || {
        analyze_pdf_with_progress(&requested_path, |update| {
            let _ = on_progress.send(update);
        })
    })
    .await
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
                u64::from(target),
                settings,
                cancel_flag,
                &mut progress,
            ),
            None => compress_pdf_with_progress(&requested_path, settings, cancel_flag, progress),
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
            optimize_images: request.optimize_images,
            compress_streams: request.compress_streams,
            strip_metadata: request.strip_metadata.or(request.remove_metadata),
            // No override-level flags here — grayscale / bilevel codec can
            // still arrive via the settings payload.
            grayscale: None,
            bilevel_codec: None,
            subset_fonts: None,
            output_dir: request.output_dir,
        },
    );

    run_compression(
        CompressionJob {
            requested_path,
            task_id,
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
            // Grayscale is a real RGB→Luma re-encode inside the engine now —
            // no longer a reason to skip image optimization entirely.
            optimize_images: Some(true),
            compress_streams: Some(true),
            grayscale: request.grayscale,
            bilevel_codec: request
                .bilevel_codec
                .as_deref()
                .map(|value| BilevelCodec::from_optional_str(Some(value))),
            subset_fonts: None,
            strip_metadata: request.strip_metadata.or(request.remove_metadata),
            output_dir: request.output_dir,
        },
    );

    run_compression(
        CompressionJob {
            requested_path,
            task_id,
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
pub fn existing_paths(paths: Vec<String>) -> Result<Vec<String>, AppErrorPayload> {
    Ok(paths
        .into_iter()
        .filter(|path| !path.trim().is_empty() && Path::new(path).exists())
        .collect())
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

    Ok(())
}

#[cfg(test)]
mod tests {
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
    fn preset_config_roundtrips_and_clears() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("preset-user-config.json");

        // Missing file reads as the default config.
        let empty = read_preset_config_at(&config_path).expect("read missing");
        assert!(empty.presets.is_empty());

        let mut config = PresetUserConfigPayload::default();
        config.presets.insert(
            "balanced".to_string(),
            pdf_core::models::PresetProfilePayload {
                image_quality: 72,
                max_image_size_percent: 80,
            },
        );
        write_preset_config_at(&config_path, &config).expect("write");

        let loaded = read_preset_config_at(&config_path).expect("read");
        assert_eq!(loaded.version, config.version);
        assert_eq!(
            loaded.presets.get("balanced").map(|p| p.image_quality),
            Some(72)
        );
        // The atomic-write temp file never lingers.
        assert!(!config_path.with_extension("tmp").exists());

        clear_preset_config_at(&config_path).expect("clear");
        assert!(!config_path.exists());
        // Clearing again is fine.
        clear_preset_config_at(&config_path).expect("clear missing");
    }

    #[test]
    fn preset_config_rejects_corrupt_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("preset-user-config.json");
        fs::write(&config_path, b"{ not json").expect("write");

        assert!(read_preset_config_at(&config_path).is_err());
    }

    #[test]
    fn existing_paths_command_filters_missing_and_blank_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.pdf");
        fs::write(&file, b"%PDF-1.5").expect("write");

        let kept = existing_paths(vec![
            file.to_string_lossy().into_owned(),
            "/definitely/missing.pdf".to_string(),
            "   ".to_string(),
        ])
        .expect("existing_paths");

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0], file.to_string_lossy());
    }
}
