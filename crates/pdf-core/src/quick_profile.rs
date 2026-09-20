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
    fs,
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

    let mut profile = serde_json::from_str::<QuickProfilePayload>(&contents).map_err(|error| {
        AppError::Config(format!(
            "Failed to parse quick profile at {}: {error}",
            config_path.display()
        ))
    })?;
    // v1→v2: the 0.7.x CMYK default-off state must not pin the upgraded
    // install to the old default (see `migrate_cmyk_default_flip`). v2→v3:
    // the ≤0.9 bilevel "jpeg" default gets the same treatment (G4 becomes
    // the default in 0.10.0). The stamped version persists on the next save.
    profile.version =
        crate::models::migrate_cmyk_default_flip(profile.version, &mut profile.cmyk_conversion);
    profile.version =
        crate::models::migrate_bilevel_default_flip(profile.version, &mut profile.bilevel_codec);
    Ok(profile)
}

/// Serialize, write through an exclusive-create temp file (pid-suffixed,
/// O_EXCL — the same discipline as the compressor's output writer), fsync,
/// then atomically rename over the target.
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
    let payload = serde_json::to_vec_pretty(profile)
        .map_err(|error| AppError::Config(format!("Failed to serialize quick profile: {error}")))?;

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

/// View the profile as settings overrides (the fallback layer below explicit
/// CLI flags, above the engine defaults).
pub fn quick_profile_overrides(profile: &QuickProfilePayload) -> CompressionSettingsOverrides {
    CompressionSettingsOverrides {
        preset: profile.preset.clone(),
        image_quality: profile.image_quality,
        // Percent wins when both spellings are present (0.9.0+ saves the
        // percent form; the px field only survives in pre-0.9.0 profiles).
        max_image_size_px: if profile.max_image_size_percent.is_some() {
            None
        } else {
            profile.max_image_size_px
        },
        max_image_size_percent: profile.max_image_size_percent.map(u16::from),
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
        assert_eq!(profile.version, 3);
        assert!(profile.preset.is_none());
    }

    #[test]
    fn v1_profile_migrates_the_cmyk_default_flip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(QUICK_PROFILE_FILE_NAME);

        // A 0.7.x profile persisted cmykConversion verbatim — false was the
        // old default state (indistinguishable from untouched) and must not
        // pin the upgrade to it; true was a deliberate opt-in and survives.
        fs::write(
            &path,
            r#"{ "version": 1, "cmykConversion": false, "grayscale": true }"#,
        )
        .expect("write v1");
        let migrated = read_quick_profile_at(&path).expect("read v1");
        assert_eq!(
            migrated.version, 3,
            "v1 profiles are stamped through to v3 in memory"
        );
        assert_eq!(
            migrated.cmyk_conversion, None,
            "old default-off resets to unset"
        );
        assert_eq!(migrated.grayscale, Some(true), "unrelated fields survive");

        fs::write(
            &path,
            r#"{ "version": 1, "cmykConversion": true, "grayscale": true }"#,
        )
        .expect("write v1 opt-in");
        let opt_in = read_quick_profile_at(&path).expect("read v1 opt-in");
        assert_eq!(opt_in.version, 3);
        assert_eq!(
            opt_in.cmyk_conversion,
            Some(true),
            "deliberate opt-ins survive"
        );
    }

    #[test]
    fn v2_profile_migrates_the_bilevel_default_flip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(QUICK_PROFILE_FILE_NAME);

        // A ≤0.9 profile persisted bilevelCodec "jpeg" verbatim — the old
        // default state (indistinguishable from untouched) and must not pin
        // the upgrade to JPEG; "ccitt-g4" was a deliberate opt-in.
        fs::write(
            &path,
            r#"{ "version": 2, "bilevelCodec": "jpeg", "grayscale": true }"#,
        )
        .expect("write v2");
        let migrated = read_quick_profile_at(&path).expect("read v2");
        assert_eq!(migrated.version, 3, "v2 profiles are stamped to v3");
        assert_eq!(
            migrated.bilevel_codec, None,
            "old default jpeg resets to unset (new default G4 applies)"
        );
        assert_eq!(migrated.grayscale, Some(true), "unrelated fields survive");

        fs::write(&path, r#"{ "version": 2, "bilevelCodec": "ccitt-g4" }"#)
            .expect("write v2 opt-in");
        let opt_in = read_quick_profile_at(&path).expect("read v2 opt-in");
        assert_eq!(opt_in.version, 3);
        assert_eq!(
            opt_in.bilevel_codec,
            Some("ccitt-g4".to_string()),
            "deliberate opt-ins survive"
        );
    }

    #[test]
    fn v3_profile_keeps_genuine_opt_outs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(QUICK_PROFILE_FILE_NAME);

        // A false/jpeg written by 0.10.0+ is a genuine opt-out: no migration
        // may touch it (otherwise every re-save could silently re-enable it).
        fs::write(
            &path,
            r#"{ "version": 3, "cmykConversion": false, "bilevelCodec": "jpeg" }"#,
        )
        .expect("write v3");
        let profile = read_quick_profile_at(&path).expect("read v3");
        assert_eq!(profile.version, 3);
        assert_eq!(profile.cmyk_conversion, Some(false));
        assert_eq!(profile.bilevel_codec, Some("jpeg".to_string()));

        // Files without a version field read as current (serde default 3),
        // so hand-written minimal profiles never re-run the migration.
        fs::write(&path, r#"{ "grayscale": true }"#).expect("write versionless");
        let versionless = read_quick_profile_at(&path).expect("read versionless");
        assert_eq!(versionless.version, 3);
        assert_eq!(versionless.cmyk_conversion, None);
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
        // The pid-suffixed atomic-write temp files never linger.
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
        assert!(overrides
            .bilevel_codec
            .is_some_and(|codec| codec.uses_ccitt()));
        assert_eq!(overrides.subset_fonts, Some(true));
        assert!(overrides.output_dir.is_none());
    }
}
