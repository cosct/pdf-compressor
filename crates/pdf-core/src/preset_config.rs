//! Preset user-config persistence — the per-preset overrides the desktop
//! shells save from their settings page.
//! 预设用户配置持久化 —— 桌面壳设置页保存的各预设覆盖值。
//!
//! Lives in the engine crate (not in a shell) so the Tauri shell and the GPUI
//! shell read and write the same file with the same migration rules.
//! 放在引擎 crate 而非某个壳里，Tauri 壳与 GPUI 壳读写同一文件、共用同一套
//! 迁移规则。

use std::{
    fs::{self, File},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{error::AppError, models::PresetUserConfigPayload};

pub const PRESET_CONFIG_FILE_NAME: &str = "preset-user-config.json";

/// `<os-config-dir>/pdf-compressor`, created on demand.
fn app_config_dir() -> Result<PathBuf, AppError> {
    let base = dirs_next::config_dir().ok_or_else(|| {
        AppError::Config("Unable to determine OS application config directory.".to_string())
    })?;
    let dir = base.join("pdf-compressor");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Where the preset config lives for this process. Unlike
/// [`crate::quick_profile::default_quick_profile_path`] this has side
/// effects: it creates the config directory and migrates a legacy file.
///
/// Try the OS config directory first (predictable, multi-user safe, works on
/// read-only install locations), falling back to the install directory
/// (portable mode) only when the config directory is unavailable. A legacy
/// install-dir config is migrated once.
pub fn resolve_preset_config_path() -> Result<PathBuf, AppError> {
    if let Ok(config_dir) = app_config_dir() {
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

/// Read the config at `config_path`. A missing or empty file yields the
/// default (no overrides) config; corrupt JSON is an error so the shell can
/// surface it instead of silently discarding the user's presets.
pub fn read_preset_config_at(config_path: &Path) -> Result<PresetUserConfigPayload, AppError> {
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
    // share the quick profile's migration rules; only the field values
    // matter per entry, the stamped version is tracked by the container.
    for profile in config.presets.values_mut() {
        let _ =
            crate::models::migrate_cmyk_default_flip(config.version, &mut profile.cmyk_conversion);
        let _ =
            crate::models::migrate_bilevel_default_flip(config.version, &mut profile.bilevel_codec);
    }
    config.version = config.version.max(3);
    Ok(config)
}

/// Serialize and atomically replace via the shared owned-temp writer — the
/// same discipline as the compressor's output writer. Failed writes preserve
/// the previous configuration.
pub fn write_preset_config_at(
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

    crate::atomic_file::write(config_path, &payload)
}

/// Delete the config at `config_path`; a missing file is not an error.
pub fn clear_preset_config_at(config_path: &Path) -> Result<(), AppError> {
    match fs::remove_file(config_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::Io(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PresetProfilePayload;

    #[test]
    fn preset_config_roundtrips_and_clears() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join(PRESET_CONFIG_FILE_NAME);

        // Missing file reads as the default config.
        let empty = read_preset_config_at(&config_path).expect("read missing");
        assert!(empty.presets.is_empty());

        let mut config = PresetUserConfigPayload::default();
        config.presets.insert(
            "balanced".to_string(),
            PresetProfilePayload {
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
        let config_path = dir.path().join(PRESET_CONFIG_FILE_NAME);
        fs::write(&config_path, b"{ not json").expect("write");

        assert!(read_preset_config_at(&config_path).is_err());
    }

    #[test]
    fn preset_config_v1_migrates_the_cmyk_default_flip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join(PRESET_CONFIG_FILE_NAME);

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
        let config_path = dir.path().join(PRESET_CONFIG_FILE_NAME);

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
        let config_path = dir.path().join(PRESET_CONFIG_FILE_NAME);

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
        let config_path = dir.path().join(PRESET_CONFIG_FILE_NAME);

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
}
