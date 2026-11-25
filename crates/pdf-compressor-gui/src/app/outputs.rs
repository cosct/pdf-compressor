use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use pdf_core::AppError;

#[derive(Debug, Clone, Default)]
pub struct SessionOutputs {
    paths: Arc<Mutex<HashSet<PathBuf>>>,
}

impl SessionOutputs {
    fn canonical(path: &Path) -> PathBuf {
        path.canonicalize().unwrap_or_else(|_| path.to_owned())
    }

    pub fn register(&self, path: impl AsRef<Path>) {
        if let Ok(mut paths) = self.paths.lock() {
            paths.insert(Self::canonical(path.as_ref()));
        }
    }

    pub fn contains(&self, path: impl AsRef<Path>) -> bool {
        self.paths
            .lock()
            .map(|paths| paths.contains(&Self::canonical(path.as_ref())))
            .unwrap_or(false)
    }

    pub fn open(&self, path: impl AsRef<Path>) -> Result<(), AppError> {
        let path = path.as_ref();
        if !self.contains(path) {
            return Err(AppError::Config(
                "Only files produced during this session can be opened.".into(),
            ));
        }
        let canonical = path
            .canonicalize()
            .map_err(|_| AppError::MissingInput(path.to_owned()))?;
        opener::open(canonical).map_err(|error| AppError::Opener(error.to_string()))
    }

    pub fn reveal(&self, path: impl AsRef<Path>) -> Result<(), AppError> {
        let path = path.as_ref();
        if !self.contains(path) {
            return Err(AppError::Config(
                "Only files produced during this session can be revealed.".into(),
            ));
        }
        let canonical = path
            .canonicalize()
            .map_err(|_| AppError::MissingInput(path.to_owned()))?;
        opener::reveal(canonical).map_err(|error| AppError::Opener(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_accepts_only_registered_outputs_and_canonicalizes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("output.pdf");
        std::fs::write(&file, b"%PDF-1.5").unwrap();
        let registry = SessionOutputs::default();
        assert!(!registry.contains(&file));
        registry.register(&file);
        assert!(registry.contains(&file));
        assert!(!registry.contains(dir.path().join("other.pdf")));
    }
}
