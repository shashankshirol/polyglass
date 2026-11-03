use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::{Manifest, ModelEntry, ModelError, Result, verify_file};

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
        dir_size(&self.root)
    }

    /// Bytes one model uses, including a partial download that would resume (MDL-03, MDL-05).
    pub fn model_disk_usage(&self, model: &ModelEntry) -> io::Result<u64> {
        dir_size(&self.model_dir(model))
    }

    /// Install a model from local files in `dir`, verified exactly like a download (MDL-04).
    /// `dir` holds the model's files under their manifest names, or in a layout
    /// [`ModelStore::import_found`] knows.
    pub fn import_from_dir(&self, model: &ModelEntry, dir: &Path) -> Result<()> {
        let layout = Layout::of(model, dir, true).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{}: not all of its files are in {}",
                    model.id,
                    dir.display()
                ),
            )
        })?;
        self.import_files(model, |name| layout.path(dir, model, name))
    }

    /// Install every manifest model whose files are all in `dir` (MDL-04), in any of the
    /// layouts a user has them in: release assets downloaded into one folder (`<id>.<file>`, as
    /// the manifest URLs name them), one folder per model (`<id>/<file>`, as
    /// `scripts/models/opus_mt.py` writes them), or the model's own folder (`dir` named `<id>`).
    /// Each file is verified against its SHA-256 exactly like a download; a model whose files
    /// aren't all there is left out. Returns each model found with its outcome.
    pub fn import_found<'m>(
        &self,
        manifest: &'m Manifest,
        dir: &Path,
    ) -> Vec<(&'m ModelEntry, Result<()>)> {
        importable(manifest, dir)
            .into_iter()
            .map(|m| (m, self.import_from_dir(m, dir)))
            .collect()
    }

    fn import_files(&self, model: &ModelEntry, path_of: impl Fn(&str) -> PathBuf) -> Result<()> {
        fs::create_dir_all(self.model_dir(model))?;
        for f in &model.files {
            let part = self.part_path(model, &f.name);
            fs::copy(path_of(&f.name), &part)?;
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

/// Where a model's files are in an import folder (see [`ModelStore::import_found`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// `<dir>/<id>.<file>`: release assets.
    Assets,
    /// `<dir>/<id>/<file>`.
    Folders,
    /// `<dir>/<file>`: the model's own folder.
    Own,
}

impl Layout {
    fn path(self, dir: &Path, model: &ModelEntry, file: &str) -> PathBuf {
        match self {
            Layout::Assets => dir.join(format!("{}.{file}", model.id)),
            Layout::Folders => dir.join(&model.id).join(file),
            Layout::Own => dir.join(file),
        }
    }

    /// How `model`'s files are laid out in `dir`, if they are all there; only names from the
    /// manifest are looked at. Plain file names count when `own` is set or `dir` is named after
    /// the model (models share file names, so a plain folder can't say whose files it holds).
    fn of(model: &ModelEntry, dir: &Path, own: bool) -> Option<Self> {
        let own = own || dir.file_name().and_then(|n| n.to_str()) == Some(model.id.as_str());
        let mut layouts = vec![Layout::Assets, Layout::Folders];
        if own {
            layouts.insert(0, Layout::Own);
        }
        layouts.into_iter().find(|l| {
            model
                .files
                .iter()
                .all(|f| l.path(dir, model, &f.name).is_file())
        })
    }
}

/// The manifest models whose files are all in `dir`, in a layout [`ModelStore::import_found`]
/// knows.
pub fn importable<'m>(manifest: &'m Manifest, dir: &Path) -> Vec<&'m ModelEntry> {
    manifest
        .models
        .iter()
        .filter(|m| Layout::of(m, dir, false).is_some())
        .collect()
}

/// Bytes under `p`; 0 when it doesn't exist.
fn dir_size(p: &Path) -> io::Result<u64> {
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
    match walk(p) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(0),
        r => r,
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
        // The file plus the marker (holding the version).
        assert_eq!(store.model_disk_usage(&m).unwrap(), 5);

        store.delete(&m).unwrap();
        assert!(!store.is_installed(&m));
        assert_eq!(store.disk_usage().unwrap(), 0);
        assert_eq!(store.model_disk_usage(&m).unwrap(), 0);
        store.delete(&m).unwrap(); // idempotent
    }

    /// Two models with two 4-byte files each ("test", so both match `HASH`).
    fn two_models() -> Manifest {
        let model = |id: &str| ModelEntry {
            id: id.into(),
            files: vec![
                ModelFile {
                    name: "model.bin".into(),
                    url: None,
                    size: 4,
                    sha256: HASH.into(),
                },
                ModelFile {
                    name: "config.json".into(),
                    url: None,
                    size: 4,
                    sha256: HASH.into(),
                },
            ],
            ..model()
        };
        Manifest {
            schema_version: 1,
            models: vec![model("opus-mt-sv-en"), model("opus-mt-en-sv")],
        }
    }

    fn write(dir: &Path, names: &[&str], body: &[u8]) {
        for n in names {
            let p = dir.join(n);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
    }

    fn found(r: &[(&ModelEntry, Result<()>)]) -> Vec<(String, bool)> {
        r.iter().map(|(m, r)| (m.id.clone(), r.is_ok())).collect()
    }

    #[test]
    fn mdl04_imports_release_assets_from_one_folder() {
        let manifest = two_models();
        let root = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        // Downloaded release assets (`<id>.<file>`), one model complete, the other missing a file.
        write(
            src.path(),
            &[
                "opus-mt-sv-en.model.bin",
                "opus-mt-sv-en.config.json",
                "opus-mt-en-sv.model.bin",
            ],
            b"test",
        );
        let r = store.import_found(&manifest, src.path());
        assert_eq!(found(&r), [("opus-mt-sv-en".into(), true)]);
        assert!(store.is_installed(&manifest.models[0]));
        assert!(!store.is_installed(&manifest.models[1]));
        assert!(root.path().join("opus-mt-sv-en/model.bin").is_file());
    }

    #[test]
    fn mdl04_imports_model_folders_and_a_models_own_folder() {
        let manifest = two_models();
        let src = tempfile::tempdir().unwrap();
        // `opus_mt.py`'s layout: one folder per model.
        write(
            src.path(),
            &[
                "opus-mt-sv-en/model.bin",
                "opus-mt-sv-en/config.json",
                "opus-mt-en-sv/model.bin",
                "opus-mt-en-sv/config.json",
            ],
            b"test",
        );
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let r = store.import_found(&manifest, src.path());
        assert_eq!(
            found(&r),
            [
                ("opus-mt-sv-en".into(), true),
                ("opus-mt-en-sv".into(), true)
            ]
        );
        // The user picked the model's own folder.
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let r = store.import_found(&manifest, &src.path().join("opus-mt-en-sv"));
        assert_eq!(found(&r), [("opus-mt-en-sv".into(), true)]);
        assert!(store.is_installed(&manifest.models[1]));
        // A folder with nothing from the manifest.
        let empty = tempfile::tempdir().unwrap();
        write(empty.path(), &["model.bin", "notes.txt"], b"test");
        assert!(store.import_found(&manifest, empty.path()).is_empty());
    }

    #[test]
    fn mdl04_imported_files_are_verified_like_a_download() {
        let manifest = two_models();
        let src = tempfile::tempdir().unwrap();
        write(src.path(), &["opus-mt-sv-en.model.bin"], b"test");
        write(src.path(), &["opus-mt-sv-en.config.json"], b"evil");
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let r = store.import_found(&manifest, src.path());
        assert!(matches!(
            &r[0].1,
            Err(ModelError::HashMismatch { file }) if file == "config.json"
        ));
        assert!(!store.is_installed(&manifest.models[0]));
        assert!(
            !store.part_path(&manifest.models[0], "config.json").exists(),
            "the bad copy is deleted"
        );
    }
}
