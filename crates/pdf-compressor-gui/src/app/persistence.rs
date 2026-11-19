//! Best-effort GUI state persistence. Only source paths, per-job settings,
//! theme and locale are written; passwords, results, run state and selection
//! are intentionally session-only.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::{
    job::Job,
    prefs::Preferences,
    queue::{Queue, QueueMutation},
    settings::Settings,
};

pub const QUEUE_STATE_FILE_NAME: &str = "gui-state.json";
pub const QUEUE_STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedEntry {
    pub source_path: String,
    #[serde(default)]
    pub settings: Settings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PersistedGuiState {
    pub version: u32,
    pub entries: Vec<PersistedEntry>,
    pub preferences: Preferences,
}

impl Default for PersistedGuiState {
    fn default() -> Self {
        Self {
            version: QUEUE_STATE_VERSION,
            entries: Vec::new(),
            preferences: Preferences::default(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedQueue<'a> {
    version: u32,
    entries: Vec<PersistedEntryRef<'a>>,
    preferences: &'a Preferences,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedEntryRef<'a> {
    source_path: &'a str,
    settings: &'a Settings,
}

/// Legacy v0 was a bare array of entries; v1 wraps it in
/// `{ version, entries, preferences }`. Missing preferences migrate to the
/// system defaults, so the first GPUI launch does not invent a theme/locale.
pub fn parse_gui_state_json(raw: &str) -> PersistedGuiState {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return PersistedGuiState::default();
    };
    let (entries, preferences) = if value.is_array() {
        (value.as_array(), Preferences::default())
    } else if value.get("version").and_then(serde_json::Value::as_u64)
        == Some(u64::from(QUEUE_STATE_VERSION))
    {
        (
            value.get("entries").and_then(serde_json::Value::as_array),
            value
                .get("preferences")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default(),
        )
    } else {
        return PersistedGuiState::default();
    };
    PersistedGuiState {
        version: QUEUE_STATE_VERSION,
        entries: parse_entries(entries),
        preferences,
    }
}

fn parse_entries(entries: Option<&Vec<serde_json::Value>>) -> Vec<PersistedEntry> {
    entries
        .into_iter()
        .flatten()
        .filter_map(|entry| serde_json::from_value::<PersistedEntry>(entry.clone()).ok())
        .filter_map(|mut entry| {
            let trimmed = entry.source_path.trim();
            if trimmed.is_empty() || !super::format::is_pdf(Path::new(trimmed)) {
                return None;
            }
            entry.source_path = trimmed.to_owned();
            entry.settings = entry.settings.normalize();
            Some(entry)
        })
        .collect()
}

/// Queue-only compatibility helper used by legacy-array migration tests.
pub fn parse_queue_json(raw: &str) -> Vec<PersistedEntry> {
    parse_gui_state_json(raw).entries
}

pub fn gui_state_json(
    jobs: &[Job],
    preferences: &Preferences,
) -> Result<String, serde_json::Error> {
    let entries = jobs
        .iter()
        .filter(|job| super::format::is_pdf(&job.source_path))
        .filter_map(|job| {
            let source_path = job.source_path.to_str()?;
            (!source_path.trim().is_empty()).then_some(PersistedEntryRef {
                source_path,
                settings: &job.settings,
            })
        })
        .collect();
    serde_json::to_string(&PersistedQueue {
        version: QUEUE_STATE_VERSION,
        entries,
        preferences,
    })
}

pub fn queue_json(jobs: &[Job]) -> Result<String, serde_json::Error> {
    gui_state_json(jobs, &Preferences::default())
}

pub fn restore_queue(
    queue: &mut Queue,
    entries: &[PersistedEntry],
) -> (QueueMutation, Vec<super::job::JobId>) {
    let restored = entries
        .iter()
        .map(|entry| (PathBuf::from(&entry.source_path), entry.settings.clone()))
        .collect::<Vec<_>>();
    queue.add_paths(
        entries
            .iter()
            .map(|entry| PathBuf::from(&entry.source_path)),
        &restored,
    )
}

/// Session queue sits alongside `quick-profile.json` and the preset config in
/// the shared OS configuration directory, without creating it until a write.
pub fn default_queue_state_path() -> Result<PathBuf, pdf_core::AppError> {
    let profile_path = pdf_core::quick_profile::default_quick_profile_path()?;
    let parent = profile_path.parent().ok_or_else(|| {
        pdf_core::AppError::Config("Unable to resolve application config directory.".to_owned())
    })?;
    Ok(parent.join(QUEUE_STATE_FILE_NAME))
}

pub fn load_gui_state_at(path: &Path) -> Result<PersistedGuiState, std::io::Error> {
    match fs::read_to_string(path) {
        Ok(raw) => Ok(parse_gui_state_json(&raw)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(PersistedGuiState::default())
        }
        Err(error) => Err(error),
    }
}

pub fn load_queue_at(path: &Path) -> Result<Vec<PersistedEntry>, std::io::Error> {
    load_gui_state_at(path).map(|state| state.entries)
}

pub fn save_gui_state_at(
    path: &Path,
    jobs: &[Job],
    preferences: &Preferences,
) -> Result<(), pdf_core::AppError> {
    let parent = path.parent().ok_or_else(|| {
        pdf_core::AppError::Config(format!(
            "Unable to resolve parent directory for GUI state: {}",
            path.display()
        ))
    })?;
    let bytes = gui_state_json(jobs, preferences).map_err(|error| {
        pdf_core::AppError::Config(format!("Failed to serialize GUI state: {error}"))
    })?;
    fs::create_dir_all(parent)?;
    pdf_core::atomic_file::write(path, bytes.as_bytes())
}

pub fn save_queue_at(path: &Path, jobs: &[Job]) -> Result<(), pdf_core::AppError> {
    save_gui_state_at(path, jobs, &Preferences::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::app::{
        job::JobStatus,
        prefs::{LocalePreference, ThemePreference},
        settings::Preset,
    };

    #[test]
    fn writes_versioned_queue_and_omits_transient_job_fields() {
        let mut queue = Queue::new();
        let (_, ids) = queue.add_paths(
            [PathBuf::from("/tmp/a.pdf"), PathBuf::from("/tmp/note.txt")],
            &[],
        );
        queue.find_mut(ids[0]).unwrap().password = Some("secret".into());
        queue.find_mut(ids[0]).unwrap().status = JobStatus::Success;
        let preferences = Preferences {
            theme: ThemePreference::Dark,
            locale: LocalePreference::English,
        };
        let json = gui_state_json(&queue.jobs, &preferences).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["version"], QUEUE_STATE_VERSION);
        assert_eq!(value["entries"].as_array().unwrap().len(), 1);
        assert_eq!(value["preferences"]["theme"], "dark");
        assert_eq!(value["preferences"]["locale"], "english");
        assert!(value["entries"][0].get("password").is_none());
        assert!(value["entries"][0].get("status").is_none());
    }

    #[test]
    fn reads_legacy_array_and_normalizes_each_entry() {
        let entries = parse_queue_json(
            r#"[{"sourcePath":" /tmp/a.PDF ","settings":{"preset":"maximum","imageQuality":200,"outputDir":"  ","targetFileSizeMb":-1}},{"sourcePath":"/tmp/no.txt","settings":{}}]"#,
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source_path, "/tmp/a.PDF");
        assert_eq!(entries[0].settings.preset, Preset::Maximum);
        assert_eq!(entries[0].settings.image_quality, 100);
        assert_eq!(entries[0].settings.output_dir, None);
        assert_eq!(entries[0].settings.target_file_size_mb, None);
    }

    #[test]
    fn malformed_or_unknown_queue_is_ignored() {
        assert!(parse_queue_json("no json").is_empty());
        assert!(parse_queue_json(r#"{"version":2,"entries":[]}"#).is_empty());
        assert!(
            parse_queue_json(r#"{"version":1,"entries":[null,{"sourcePath":"","settings":{}}]}"#)
                .is_empty()
        );
    }

    #[test]
    fn missing_preferences_migrate_to_system_defaults() {
        let state = parse_gui_state_json(r#"{"version":1,"entries":[]}"#);
        assert_eq!(state.preferences, Preferences::default());
    }

    #[test]
    fn atomic_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gui-state.json");
        let mut queue = Queue::new();
        queue.add_paths([PathBuf::from("/tmp/a.pdf")], &[]);
        save_queue_at(&path, &queue.jobs).unwrap();
        assert_eq!(load_queue_at(&path).unwrap().len(), 1);
        assert!(dir.path().join("gui-state.json").exists());
    }
}
