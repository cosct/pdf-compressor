//! Quick-mode profile persistence — the right-click / headless compression
//! defaults shared by the desktop settings page (writer) and the
//! `pdf-compressor-cli quick` subcommand (reader).
//! 快速（右键）模式配置持久化 —— 桌面设置页写入、`pdf-compressor-cli quick`
//! 读取的共享默认参数。
//!
//! Both sides resolve the same path (`<os-config-dir>/pdf-compressor/
//! quick-profile.json`) so a profile saved in the GUI is picked up by the
//! file-manager integration without any flag passing.
//! 两端解析同一路径，GUI 保存的配置无需任何命令行参数即可被文件管理器
//! 集成读取。

use std::{
    fs::{self, File},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use crate::{
    error::AppError,
    models::QuickProfilePayload,
    pdf::{BilevelCodec, CompressionSettingsOverrides},
};

pub const QUICK_PROFILE_FILE_NAME: &str = "quick-profile.json";

/// `<os-config-dir>/pdf-compressor/quick-profile.json`.
pub fn default_quick_profile_path() -> Result<PathBuf, AppError> {
    let base = dirs_next::config_dir().ok_or_else(|| {
        AppError::Config("Unable to determine OS application config directory.".to_string())
    })?;
    Ok(base.join("pdf-compressor").join(QUICK_PROFILE_FILE_NAME))
}

/// Read the profile at `path`. A missing file yields the empty (all-default)
/// profile; corrupt JSON is an error so the GUI can surface it — the CLI
/// converts that error into a warning plus defaults.
pub fn read_quick_profile_at(config_path: &Path) -> Result<QuickProfilePayload, AppError> {
    let contents = match fs::read_to_string(config_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(QuickProfilePayload::default());
        }
        Err(error) => return Err(AppError::Io(error)),
    };

    if contents.trim().is_empty() {
        return Ok(QuickProfilePayload::default());
    }

    serde_json::from_str::<QuickProfilePayload>(&contents).map_err(|error| {
        AppError::Config(format!(
            "Failed to parse quick profile at {}: {error}",
            config_path.display()
        ))
    })
}

/// Serialize, write to a temp file, sync, then atomically rename over the
/// target (same discipline as the preset config writer).
pub fn write_quick_profile_at(
    config_path: &Path,
    profile: &QuickProfilePayload,
) -> Result<(), AppError> {
    let parent_dir = config_path.parent().ok_or_else(|| {
        AppError::Config(format!(
            "Unable to resolve parent directory for quick profile: {}",
            config_path.display()
        ))
    })?;
    let temp_path = config_path.with_extension("tmp");
    let payload = serde_json::to_vec_pretty(profile)
        .map_err(|error| AppError::Config(format!("Failed to serialize quick profile: {error}")))?;

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

/// View the profile as settings overrides (the fallback layer below explicit
/// CLI flags, above the engine defaults).
pub fn quick_profile_overrides(profile: &QuickProfilePayload) -> CompressionSettingsOverrides {
    CompressionSettingsOverrides {
        preset: profile.preset.clone(),
        image_quality: profile.image_quality,
        max_image_size_px: profile.max_image_size_px,
        optimize_images: profile.optimize_images,
        compress_streams: profile.compress_streams,
        strip_metadata: profile.strip_metadata,
        grayscale: profile.grayscale,
        bilevel_codec: profile
            .bilevel_codec
            .as_deref()
            .map(|value| BilevelCodec::from_optional_str(Some(value))),
        subset_fonts: profile.subset_fonts,
        cmyk_conversion: profile.cmyk_conversion,
        // Quick mode always writes next to the original file.
        output_dir: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_reads_as_default_profile() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(QUICK_PROFILE_FILE_NAME);

        let profile = read_quick_profile_at(&path).expect("read missing");
        assert_eq!(profile.version, 1);
        assert!(profile.preset.is_none());
    }

    #[test]
    fn profile_roundtrips_through_atomic_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(QUICK_PROFILE_FILE_NAME);

        let profile = QuickProfilePayload {
            preset: Some("maximum".to_string()),
            image_quality: Some(55),
            grayscale: Some(true),
            bilevel_codec: Some("ccitt-g4".to_string()),
            target_size_bytes: Some(5 * 1024 * 1024),
            ..Default::default()
        };
        write_quick_profile_at(&path, &profile).expect("write");

        let loaded = read_quick_profile_at(&path).expect("read");
        assert_eq!(loaded.preset.as_deref(), Some("maximum"));
        assert_eq!(loaded.image_quality, Some(55));
        assert_eq!(loaded.target_size_bytes, Some(5 * 1024 * 1024));
        assert!(!path.with_extension("tmp").exists(), "temp file never lingers");
    }

    #[test]
    fn corrupt_json_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(QUICK_PROFILE_FILE_NAME);
        fs::write(&path, b"{ not json").expect("write");

        assert!(read_quick_profile_at(&path).is_err());
    }

    #[test]
    fn profile_maps_to_overrides() {
        let profile = QuickProfilePayload {
            preset: Some("conservative".to_string()),
            grayscale: Some(true),
            bilevel_codec: Some("ccitt-g4".to_string()),
            subset_fonts: Some(true),
            ..Default::default()
        };

        let overrides = quick_profile_overrides(&profile);
        assert_eq!(overrides.preset.as_deref(), Some("conservative"));
        assert_eq!(overrides.grayscale, Some(true));
        assert!(overrides.bilevel_codec.is_some_and(|codec| codec.uses_ccitt()));
        assert_eq!(overrides.subset_fonts, Some(true));
        assert!(overrides.output_dir.is_none());
    }
}
