use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::{ModelEntry, ModelError, Result, verify_file};

/// Marker written after every file of a model has been verified.
const COMPLETE_MARKER: &str = ".complete";

/// Installed models on disk: `<root>/<model id>/<file name>`.
#[derive(Debug, Clone)]
pub struct ModelStore {
    root: PathBuf,
}

impl ModelStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn model_dir(&self, model: &ModelEntry) -> PathBuf {
        self.root.join(&model.id)
    }

    pub fn file_path(&self, model: &ModelEntry, file: &str) -> PathBuf {
        self.model_dir(model).join(file)
    }

    pub(crate) fn part_path(&self, model: &ModelEntry, file: &str) -> PathBuf {
        self.model_dir(model).join(format!("{file}.part"))
    }

    /// Installed = verified marker present and every file has its manifest size.
    pub fn is_installed(&self, model: &ModelEntry) -> bool {
        self.model_dir(model).join(COMPLETE_MARKER).is_file()
            && model.files.iter().all(|f| {
                fs::metadata(self.file_path(model, &f.name)).is_ok_and(|m| m.len() == f.size)
            })
    }

    pub(crate) fn mark_complete(&self, model: &ModelEntry) -> io::Result<()> {
        fs::write(self.model_dir(model).join(COMPLETE_MARKER), &model.version)
    }

    /// Remove a model and its partial downloads (MDL-05).
    pub fn delete(&self, model: &ModelEntry) -> io::Result<()> {
        match fs::remove_dir_all(self.model_dir(model)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }

    /// Total bytes used by all models, including partial downloads (MDL-05).
    pub fn disk_usage(&self) -> io::Result<u64> {
        fn walk(p: &Path) -> io::Result<u64> {
            let mut total = 0;
            for entry in fs::read_dir(p)? {
                let entry = entry?;
                let meta = entry.metadata()?;
                total += if meta.is_dir() {
                    walk(&entry.path())?
                } else {
                    meta.len()
                };
            }
            Ok(total)
        }
        match walk(&self.root) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(0),
            r => r,
        }
    }

    /// Install a model from local files in `dir`, verified exactly like a download (MDL-04).
    pub fn import_from_dir(&self, model: &ModelEntry, dir: &Path) -> Result<()> {
        fs::create_dir_all(self.model_dir(model))?;
        for f in &model.files {
            let part = self.part_path(model, &f.name);
            fs::copy(dir.join(&f.name), &part)?;
            if !verify_file(&part, &f.sha256)? {
                let _ = fs::remove_file(&part);
                return Err(ModelError::HashMismatch {
                    file: f.name.clone(),
                });
            }
            fs::rename(&part, self.file_path(model, &f.name))?;
        }
        self.mark_complete(model)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelFile, manifest::tests::HASH};

    fn model() -> ModelEntry {
        ModelEntry {
            id: "m".into(),
            engine: "ctranslate2".into(),
            source: pg_core::LangTag::sv(),
            target: pg_core::LangTag::en(),
            version: "1".into(),
            license: "MIT".into(),
            attribution: None,
            source_url: "https://example.org".into(),
            files: vec![ModelFile {
                name: "model.bin".into(),
                url: None,
                size: 4,
                sha256: HASH.into(),
            }],
        }
    }

    #[test]
    fn mdl04_import_verifies_and_mdl05_delete() {
        let root = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let m = model();
        assert_eq!(store.disk_usage().unwrap(), 0);

        std::fs::write(src.path().join("model.bin"), b"nope").unwrap();
        assert!(matches!(
            store.import_from_dir(&m, src.path()),
            Err(ModelError::HashMismatch { .. })
        ));
        assert!(!store.is_installed(&m));
        assert!(
            !store.part_path(&m, "model.bin").exists(),
            "bad file deleted"
        );

        std::fs::write(src.path().join("model.bin"), b"test").unwrap();
        store.import_from_dir(&m, src.path()).unwrap();
        assert!(store.is_installed(&m));
        assert!(store.disk_usage().unwrap() >= 4);

        store.delete(&m).unwrap();
        assert!(!store.is_installed(&m));
        assert_eq!(store.disk_usage().unwrap(), 0);
        store.delete(&m).unwrap(); // idempotent
    }
}
