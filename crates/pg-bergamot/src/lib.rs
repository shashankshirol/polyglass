//! On-device translation with Mozilla's Bergamot models (TR-01, ADR-0004).
//!
//! The models are the Firefox Translations students listed in `models/manifest.toml` with
//! `engine = "bergamot"`: an int8 Marian model, a SentencePiece vocabulary and a lexical
//! shortlist per direction. They run on [`fxtranslate`], a pure-Rust port of the Bergamot
//! inference engine, so this crate needs no C++ build and no `unsafe`.
//!
//! [`BergamotTranslator::load_installed`] loads every installed direction up front, so swapping
//! sv→en and en→sv never reloads anything (SV-01). Wrap it in
//! [`pg_translate::TokenGuard`] to keep numbers, URLs and e-mails intact (SV-05).

use std::fmt;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use fxtranslate::engine::Engine;
use pg_core::LangPair;
use pg_models::{Manifest, ModelEntry, ModelStore};
use pg_translate::{Result, TranslateError, Translator};

/// `engine` value of the manifest entries this crate loads.
pub const ENGINE: &str = "bergamot";

/// Largest file this crate decompresses; the sv↔en model files are under 20 MB.
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{model}: no {role} file in the manifest entry")]
    MissingRole { model: String, role: &'static str },
    #[error("{model}/{file}: {source}")]
    Io {
        model: String,
        file: String,
        source: std::io::Error,
    },
    #[error("{model}: model rejected by the engine: {reason}")]
    Engine { model: String, reason: String },
}

/// Which manifest file is which, by Bergamot's naming convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFiles {
    pub model: String,
    pub vocab: String,
    pub shortlist: String,
}

impl ModelFiles {
    /// `model.*`, `vocab.*` and `lex.*` (optionally `.gz`). Split source/target vocabularies
    /// (CJK pairs) aren't needed for Swedish and aren't supported yet.
    pub fn of(entry: &ModelEntry) -> std::result::Result<Self, LoadError> {
        let find = |prefix: &str, role: &'static str| {
            entry
                .files
                .iter()
                .find(|f| f.name.starts_with(prefix))
                .map(|f| f.name.clone())
                .ok_or_else(|| LoadError::MissingRole {
                    model: entry.id.clone(),
                    role,
                })
        };
        Ok(Self {
            model: find("model.", "model")?,
            vocab: find("vocab.", "vocabulary")?,
            shortlist: find("lex.", "shortlist")?,
        })
    }
}

/// The on-device backend: one loaded engine per installed direction.
pub struct BergamotTranslator {
    engines: Vec<(LangPair, Arc<Engine>)>,
}

impl fmt::Debug for BergamotTranslator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BergamotTranslator")
            .field("pairs", &self.pairs())
            .finish()
    }
}

impl BergamotTranslator {
    /// No models: every pair reports [`TranslateError::ModelMissing`].
    pub fn empty() -> Self {
        Self {
            engines: Vec::new(),
        }
    }

    /// Load every installed `bergamot` model in `manifest`. Models that aren't installed are
    /// skipped; an installed model that fails to load is an error.
    pub fn load_installed(
        manifest: &Manifest,
        store: &ModelStore,
    ) -> std::result::Result<Self, LoadError> {
        let mut engines = Vec::new();
        for entry in manifest.models.iter().filter(|m| m.engine == ENGINE) {
            if store.is_installed(entry) {
                engines.push((entry.pair(), Arc::new(load_model(entry, store)?)));
            }
        }
        Ok(Self { engines })
    }

    /// Directions that are loaded and ready.
    pub fn pairs(&self) -> Vec<LangPair> {
        self.engines.iter().map(|(p, _)| p.clone()).collect()
    }

    fn engine(&self, pair: &LangPair) -> Option<Arc<Engine>> {
        self.engines
            .iter()
            .find(|(p, _)| {
                p.source.same_language(&pair.source) && p.target.same_language(&pair.target)
            })
            .map(|(_, e)| e.clone())
    }
}

/// Load one installed model from the store (files as verified by `pg-models`).
pub fn load_model(
    entry: &ModelEntry,
    store: &ModelStore,
) -> std::result::Result<Engine, LoadError> {
    let files = ModelFiles::of(entry)?;
    let read = |name: &str| {
        read_maybe_gz(&store.file_path(entry, name)).map_err(|source| LoadError::Io {
            model: entry.id.clone(),
            file: name.to_owned(),
            source,
        })
    };
    let model = read(&files.model)?;
    let vocab = read(&files.vocab)?;
    let shortlist = read(&files.shortlist)?;
    let engine =
        Engine::from_bytes(&model, &vocab, &vocab).map_err(|reason| LoadError::Engine {
            model: entry.id.clone(),
            reason,
        })?;
    Ok(engine.with_shortlist_bytes(&shortlist))
}

fn read_maybe_gz(path: &Path) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let mut out = Vec::new();
    if path.extension().is_some_and(|e| e == "gz") {
        flate2::read::GzDecoder::new(file)
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut out)?;
    } else {
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut out)?;
    }
    if out.len() as u64 > MAX_FILE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file too large",
        ));
    }
    Ok(out)
}

#[async_trait]
impl Translator for BergamotTranslator {
    fn id(&self) -> &str {
        ENGINE
    }

    fn display_name(&self) -> &str {
        "Bergamot"
    }

    fn is_local(&self) -> bool {
        true
    }

    fn supports(&self, pair: &LangPair) -> bool {
        self.engine(pair).is_some()
    }

    async fn translate(&self, blocks: &[String], pair: &LangPair) -> Result<Vec<String>> {
        let engine = self
            .engine(pair)
            .ok_or_else(|| TranslateError::ModelMissing(pair.clone()))?;
        let blocks = blocks.to_vec();
        // CPU-bound; keep it off the async workers. Each block is split into sentences.
        tokio::task::spawn_blocking(move || {
            blocks
                .iter()
                .map(|b| engine.translate_long(b))
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|_| TranslateError::Backend("bergamot translation task failed".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pg_models::{ModelFile, sha256_file};

    #[test]
    fn bundled_manifest_has_both_swedish_directions() {
        let manifest = Manifest::bundled().unwrap();
        let models = manifest.swedish_english_models();
        let pairs: Vec<_> = models.iter().map(|m| m.pair()).collect();
        assert_eq!(pairs, [LangPair::sv_en(), LangPair::en_sv()]);
        for m in models {
            assert_eq!(m.engine, ENGINE);
            assert_eq!(m.license, "MPL-2.0");
            let files = ModelFiles::of(m).unwrap();
            assert!(
                files.model.ends_with(".intgemm.alphas.bin.gz"),
                "{}",
                files.model
            );
        }
        let total: u64 = manifest
            .swedish_english_models()
            .iter()
            .map(|m| m.download_size())
            .sum();
        // SIZE-05: both directions ≤ 100 MB.
        assert!(total < 100_000_000, "{total}");
    }

    #[tokio::test]
    async fn without_models_every_pair_is_model_missing() {
        let t = BergamotTranslator::load_installed(
            &Manifest::bundled().unwrap(),
            &ModelStore::new(tempfile::tempdir().unwrap().path()),
        )
        .unwrap();
        assert!(t.pairs().is_empty());
        assert!(!t.supports(&LangPair::sv_en()));
        assert!(t.is_local());
        assert_eq!(
            t.translate(&["Spara".into()], &LangPair::sv_en()).await,
            Err(TranslateError::ModelMissing(LangPair::sv_en()))
        );
    }

    #[test]
    fn a_broken_installed_model_is_an_error_not_a_panic() {
        let src = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut files = Vec::new();
        for name in [
            "model.xxyy.intgemm.alphas.bin",
            "vocab.xxyy.spm",
            "lex.xxyy.s2t.bin",
        ] {
            let path = src.path().join(name);
            std::fs::write(&path, b"not a model").unwrap();
            files.push(ModelFile {
                name: name.into(),
                url: format!("https://example.org/{name}"),
                size: 11,
                sha256: sha256_file(&path).unwrap(),
            });
        }
        let entry = ModelEntry {
            id: "bergamot-xx-yy".into(),
            engine: ENGINE.into(),
            source: pg_core::LangTag::sv(),
            target: pg_core::LangTag::en(),
            version: "1".into(),
            license: "MPL-2.0".into(),
            source_url: "https://example.org".into(),
            files,
        };
        let store = ModelStore::new(root.path());
        store.import_from_dir(&entry, src.path()).unwrap();
        let manifest = Manifest {
            schema_version: 1,
            models: vec![entry],
        };
        assert!(matches!(
            BergamotTranslator::load_installed(&manifest, &store),
            Err(LoadError::Engine { .. })
        ));
    }
}
