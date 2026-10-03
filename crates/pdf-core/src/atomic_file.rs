//! Same-directory atomic replacement. The temporary owns exactly one path;
//! failures never remove the previous destination or scan sibling files.
use std::{io::Write, path::Path};

use crate::AppError;

pub fn write(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    write_checked(path, bytes, || Ok(()))
}

/// Check cancellation after writing/syncing, immediately before committing.
/// Once persist succeeds the new file is committed and must be reported as such.
pub fn write_checked(
    path: &Path,
    bytes: &[u8],
    before_commit: impl FnOnce() -> Result<(), AppError>,
) -> Result<(), AppError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    before_commit()?;
    // persist replaces atomically on supported platforms, including Windows.
    // On error the owned temporary is dropped; the old target stays intact.
    temp.persist(path)
        .map_err(|error| AppError::Io(error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_before_commit_preserves_old_file_and_cleans_only_owned_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.pdf");
        let sibling = dir.path().join("out.pdf.user-backup.tmp");
        std::fs::write(&path, b"old").unwrap();
        std::fs::write(&sibling, b"backup").unwrap();
        let result = write_checked(&path, b"new", || Err(AppError::Cancelled("test".into())));
        assert!(matches!(result, Err(AppError::Cancelled(_))));
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(std::fs::read(&sibling).unwrap(), b"backup");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn failed_replace_preserves_destination_and_removes_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("destination");
        std::fs::create_dir(&path).unwrap();
        assert!(write(&path, b"new").is_err());
        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        write(&path, b"old").unwrap();
        write(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }
}
