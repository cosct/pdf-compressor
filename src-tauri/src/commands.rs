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
    analyze_pdf_with_progress, build_features as engine_build_features,
    compress_pdf_to_target_size, compress_pdf_with_progress,
    models::{
        AnalysisResponse, BuildFeatures, CompressPdfRequest, CompressScannedPdfRequest,
        CompressionResponse, PresetUserConfigPayload, ProgressUpdate, QuickProfilePayload,
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

    let mut config =
        serde_json::from_str::<PresetUserConfigPayload>(&contents).map_err(|error| {
            AppError::Config(format!(
                "Failed to parse preset config at {}: {error}",
                config_path.display()
            ))
        })?;
    // v1→v2: the 0.7.x CMYK default-off state must not pin the upgraded
    // install to the old default. v2→v3: the ≤0.9 bilevel "jpeg" default
    // likewise resets so 0.10.0's G4 default applies. The per-profile calls
    // share the engine's migration rules; only the field values matter per
    // entry, the stamped version is tracked by the container.
    for profile in config.presets.values_mut() {
        let _ = pdf_core::migrate_cmyk_default_flip(config.version, &mut profile.cmyk_conversion);
        let _ = pdf_core::migrate_bilevel_default_flip(config.version, &mut profile.bilevel_codec);
    }
    config.version = config.version.max(3);
    Ok(config)
}

fn write_preset_user_config(config: &PresetUserConfigPayload) -> Result<(), AppError> {
    write_preset_config_at(&resolve_preset_config_path()?, config)
}

/// Path-parameterized core of [`write_preset_user_config`]: serialize, write
/// to an exclusive-create temp file, sync, then atomically rename over the
/// target — the same discipline as the compressor's output writer.
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
    let payload = serde_json::to_vec_pretty(config)
        .map_err(|error| AppError::Config(format!("Failed to serialize preset config: {error}")))?;

    fs::create_dir_all(parent_dir)?;

    // create_new (O_EXCL): a predictable `.tmp` name created with truncation
    // would clobber a same-named file (or a symlink planted in a writable
    // directory). The pid suffix makes collisions essentially impossible;
    // the retry loop closes the rest.
    let mut attempt = 0u32;
    loop {
        let suffix = if attempt == 0 {
            format!("{}.tmp", std::process::id())
        } else {
            format!("{}.{}.tmp", std::process::id(), attempt)
        };
        let temp_path = config_path.with_extension(suffix);
        let mut temp = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(temp) => temp,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                attempt += 1;
                if attempt >= 4 {
                    return Err(AppError::Io(error));
                }
                continue;
            }
            Err(error) => return Err(AppError::Io(error)),
        };
        let result = (|| -> Result<(), std::io::Error> {
            temp.write_all(&payload)?;
            temp.sync_all()?;
            drop(temp);
            // Atomic on Unix; on Windows, try rename-over first (NTFS
            // supports it), falling back to remove-then-rename for older
            // filesystems.
            if fs::rename(&temp_path, config_path).is_err() {
                let _ = fs::remove_file(config_path);
                fs::rename(&temp_path, config_path)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temp_path);
            return Err(AppError::Io(error));
        }
        return Ok(());
    }
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
pub async fn save_preset_user_config(
    config: PresetUserConfigPayload,
) -> Result<PresetUserConfigPayload, AppErrorPayload> {
    // The writer fsyncs (write_all + sync_all) — keep that off the main
    // thread; sync commands run on it in Tauri v2.
    tauri::async_runtime::spawn_blocking(move || {
        write_preset_user_config(&config).map_err(AppErrorPayload::from)?;
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
    clear_preset_user_config_file().map_err(AppErrorPayload::from)
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
            cmyk_conversion: None,
            strip_metadata: request.strip_metadata.or(request.remove_metadata),
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
                ..Default::default()
            },
        );
        write_preset_config_at(&config_path, &config).expect("write");

        let loaded = read_preset_config_at(&config_path).expect("read");
        assert_eq!(loaded.version, config.version);
        assert_eq!(
            loaded.presets.get("balanced").map(|p| p.image_quality),
            Some(72)
        );
        // The atomic-write temp files (pid-suffixed) never linger.
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files never linger: {leftovers:?}"
        );

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
    fn preset_config_v1_migrates_the_cmyk_default_flip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("preset-user-config.json");

        // A 0.7.x config persisted cmykConversion verbatim: false was the
        // old default state and must not pin the upgrade to it; true was a
        // deliberate opt-in and survives. Unrelated fields ride along.
        fs::write(
            &config_path,
            r#"{
                "version": 1,
                "presets": {
                    "balanced":  { "imageQuality": 72, "maxImageSizePercent": 80, "cmykConversion": false },
                    "maximum":   { "imageQuality": 46, "maxImageSizePercent": 52, "cmykConversion": true },
                    "custom":    { "imageQuality": 60, "maxImageSizePercent": 70 }
                }
            }"#,
        )
        .expect("write v1 config");

        let migrated = read_preset_config_at(&config_path).expect("read v1");
        assert_eq!(
            migrated.version, 3,
            "v1 configs are stamped through to v3 in memory"
        );
        assert_eq!(
            migrated.presets["balanced"].cmyk_conversion, None,
            "old default-off resets to unset (new default applies)"
        );
        assert_eq!(
            migrated.presets["maximum"].cmyk_conversion,
            Some(true),
            "deliberate opt-ins survive"
        );
        assert_eq!(
            migrated.presets["custom"].cmyk_conversion, None,
            "untouched entries stay unset"
        );
        assert_eq!(migrated.presets["balanced"].image_quality, 72);
    }

    #[test]
    fn preset_config_v2_keeps_genuine_opt_outs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("preset-user-config.json");

        // A false written by 0.8.0+ is a genuine opt-out and must survive
        // every future load untouched.
        fs::write(
            &config_path,
            r#"{ "version": 2, "presets": { "balanced": { "imageQuality": 72, "maxImageSizePercent": 80, "cmykConversion": false } } }"#,
        )
        .expect("write v2 config");

        let loaded = read_preset_config_at(&config_path).expect("read v2");
        assert_eq!(loaded.version, 3);
        assert_eq!(loaded.presets["balanced"].cmyk_conversion, Some(false));
    }

    #[test]
    fn preset_config_v2_migrates_the_bilevel_default_flip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("preset-user-config.json");

        // A ≤0.9 config materialized the then-default "jpeg" into every
        // preset — indistinguishable from untouched, so it must reset for
        // the 0.10.0 G4 default; "ccitt-g4" was a deliberate opt-in.
        fs::write(
            &config_path,
            r#"{
                "version": 2,
                "presets": {
                    "balanced":  { "imageQuality": 72, "maxImageSizePercent": 68, "bilevelCodec": "jpeg" },
                    "maximum":   { "imageQuality": 46, "maxImageSizePercent": 52, "bilevelCodec": "ccitt-g4" },
                    "custom":    { "imageQuality": 60, "maxImageSizePercent": 70 }
                }
            }"#,
        )
        .expect("write v2 config");

        let migrated = read_preset_config_at(&config_path).expect("read v2");
        assert_eq!(migrated.version, 3, "v2 configs are stamped to v3");
        assert_eq!(
            migrated.presets["balanced"].bilevel_codec, None,
            "old default jpeg resets to unset (G4 default applies)"
        );
        assert_eq!(
            migrated.presets["maximum"].bilevel_codec,
            Some("ccitt-g4".to_string()),
            "deliberate opt-ins survive"
        );
        assert_eq!(
            migrated.presets["custom"].bilevel_codec, None,
            "untouched entries stay unset"
        );
    }

    #[test]
    fn preset_config_v3_keeps_genuine_opt_outs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("preset-user-config.json");

        // A jpeg written by 0.10.0+ is a genuine opt-out: no migration may
        // touch it (otherwise every re-save could silently re-enable G4).
        fs::write(
            &config_path,
            r#"{ "version": 3, "presets": { "balanced": { "imageQuality": 72, "maxImageSizePercent": 68, "bilevelCodec": "jpeg" } } }"#,
        )
        .expect("write v3 config");

        let loaded = read_preset_config_at(&config_path).expect("read v3");
        assert_eq!(loaded.version, 3);
        assert_eq!(
            loaded.presets["balanced"].bilevel_codec,
            Some("jpeg".to_string())
        );
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
