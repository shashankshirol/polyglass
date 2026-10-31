//! On-device translation with CTranslate2 and OPUS-MT models (TR-01, ADR-0005).
//!
//! Every model in `models/manifest.toml` with `engine = "ctranslate2"` is a CTranslate2 model
//! directory: `model.bin`, `config.json` and `shared_vocabulary.json` from the converter, plus the
//! SentencePiece models `source.spm` and `target.spm`. OPUS-MT's Marian models are converted to
//! int8 by `scripts/models/opus_mt.py`. Nothing here is specific to a language pair: a new pair
//! (M3-8, M3-9) needs only a manifest entry.
//!
//! [`Ct2Translator::load_installed`] loads every installed direction up front, so swapping
//! sv→en and en→sv never reloads anything (SV-01). Wrap it in [`pg_translate::TokenGuard`] to
//! keep numbers, URLs and e-mails intact (SV-05). CTranslate2 itself is C++, built by `ct2rs`;
//! its `cxx` bridge keeps `unsafe` out of this crate.

mod sentences;

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use ct2rs::tokenizers::sentencepiece::Tokenizer;
use ct2rs::{ComputeType, Config, TranslationOptions};
use pg_core::LangPair;
use pg_models::{Manifest, ModelEntry, ModelStore};
use pg_translate::{Result, TranslateError, Translator};

pub use sentences::split_sentences;

/// `engine` value of the manifest entries this crate loads.
pub const ENGINE: &str = "ctranslate2";

/// Files every model directory must have, as the manifest lists them.
pub const MODEL_FILES: [&str; 5] = [
    "model.bin",
    "config.json",
    "shared_vocabulary.json",
    "source.spm",
    "target.spm",
];

/// Most threads a model uses by default (see [`Options::default_threads`]).
pub const MAX_DEFAULT_THREADS: usize = 4;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{model}: no {file} in the manifest entry")]
    MissingFile { model: String, file: &'static str },
    #[error("{model}: model rejected by CTranslate2: {reason}")]
    Engine { model: String, reason: String },
}

/// How models are loaded and run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// CPU threads per model. Only one direction runs at a time, so this is also the most
    /// translation uses at once.
    pub threads: usize,
    /// 1 = greedy decoding.
    pub beam_size: usize,
    /// Longest output per sentence, in tokens.
    pub max_decoding_length: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            threads: Self::default_threads(),
            beam_size: 1,
            max_decoding_length: 256,
        }
    }
}

impl Options {
    /// Half the logical CPUs, at least 1 and at most [`MAX_DEFAULT_THREADS`], so translating
    /// under the lens leaves the rest of a small machine free. CTranslate2 needs a few threads
    /// to be fast: 4 threads take a paragraph from ≈ 650 ms to ≈ 200 ms on the dev machine
    /// (ADR-0005).
    pub fn default_threads() -> usize {
        let cpus = std::thread::available_parallelism().map_or(2, |n| n.get());
        (cpus / 2).clamp(1, MAX_DEFAULT_THREADS)
    }
}

/// One loaded direction.
struct Model {
    pair: LangPair,
    id: String,
    translator: ct2rs::Translator<Tokenizer>,
}

/// The on-device backend: one loaded model per installed direction.
pub struct Ct2Translator {
    models: Vec<Arc<Model>>,
    options: Options,
}

impl fmt::Debug for Ct2Translator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ct2Translator")
            .field(
                "models",
                &self.models.iter().map(|m| &m.id).collect::<Vec<_>>(),
            )
            .field("options", &self.options)
            .finish()
    }
}

impl Ct2Translator {
    /// No models: every pair reports [`TranslateError::ModelMissing`].
    pub fn empty() -> Self {
        Self {
            models: Vec::new(),
            options: Options::default(),
        }
    }

    /// Load every installed `ctranslate2` model in `manifest`. Models that aren't installed are
    /// skipped; an installed model that fails to load is an error.
    pub fn load_installed(
        manifest: &Manifest,
        store: &ModelStore,
        options: Options,
    ) -> std::result::Result<Self, LoadError> {
        let mut models = Vec::new();
        for entry in manifest.models.iter().filter(|m| m.engine == ENGINE) {
            if store.is_installed(entry) {
                models.push(Arc::new(load_model(entry, store, &options)?));
            }
        }
        Ok(Self { models, options })
    }

    /// Directions that are loaded and ready.
    pub fn pairs(&self) -> Vec<LangPair> {
        self.models.iter().map(|m| m.pair.clone()).collect()
    }

    pub fn options(&self) -> &Options {
        &self.options
    }

    fn model(&self, pair: &LangPair) -> Option<Arc<Model>> {
        self.models
            .iter()
            .find(|m| {
                m.pair.source.same_language(&pair.source)
                    && m.pair.target.same_language(&pair.target)
            })
            .cloned()
    }
}

/// Load one installed model from the store (files as verified by `pg-models`).
fn load_model(
    entry: &ModelEntry,
    store: &ModelStore,
    options: &Options,
) -> std::result::Result<Model, LoadError> {
    for file in MODEL_FILES {
        if !entry.files.iter().any(|f| f.name == file) {
            return Err(LoadError::MissingFile {
                model: entry.id.clone(),
                file,
            });
        }
    }
    let dir = store.model_dir(entry);
    let engine = |reason: String| LoadError::Engine {
        model: entry.id.clone(),
        reason,
    };
    let tokenizer = Tokenizer::new(&dir).map_err(|e| engine(e.to_string()))?;
    let config = Config {
        compute_type: ComputeType::INT8,
        num_threads_per_replica: options.threads.max(1),
        ..Default::default()
    };
    let translator = ct2rs::Translator::with_tokenizer(&dir, tokenizer, &config)
        .map_err(|e| engine(e.to_string()))?;
    Ok(Model {
        pair: entry.pair(),
        id: entry.id.clone(),
        translator,
    })
}

impl Model {
    /// Translate blocks. Each block is split into sentences (OPUS-MT is trained on sentences)
    /// and all sentences go to CTranslate2 as one batch.
    fn translate(
        &self,
        blocks: &[String],
        options: &Options,
    ) -> std::result::Result<Vec<String>, String> {
        let split: Vec<Vec<&str>> = blocks.iter().map(|b| split_sentences(b)).collect();
        let sentences: Vec<&str> = split.iter().flatten().copied().collect();
        if sentences.is_empty() {
            return Ok(blocks.to_vec());
        }
        let opts = TranslationOptions::<String, String> {
            beam_size: options.beam_size.max(1),
            max_decoding_length: options.max_decoding_length,
            ..Default::default()
        };
        let out = self
            .translator
            .translate_batch(&sentences, &opts, None)
            .map_err(|e| e.to_string())?;
        let mut out = out.into_iter().map(|(text, _)| text);
        Ok(blocks
            .iter()
            .zip(&split)
            .map(|(block, parts)| {
                if parts.is_empty() {
                    return block.clone();
                }
                parts
                    .iter()
                    .map(|_| out.next().unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect())
    }
}

#[async_trait]
impl Translator for Ct2Translator {
    fn id(&self) -> &str {
        ENGINE
    }

    fn display_name(&self) -> &str {
        "CTranslate2"
    }

    fn is_local(&self) -> bool {
        true
    }

    fn supports(&self, pair: &LangPair) -> bool {
        self.model(pair).is_some()
    }

    async fn translate(&self, blocks: &[String], pair: &LangPair) -> Result<Vec<String>> {
        let model = self
            .model(pair)
            .ok_or_else(|| TranslateError::ModelMissing(pair.clone()))?;
        let blocks = blocks.to_vec();
        let options = self.options.clone();
        // CPU-bound; keep it off the async workers.
        tokio::task::spawn_blocking(move || model.translate(&blocks, &options))
            .await
            .map_err(|_| TranslateError::Backend("ctranslate2 translation task failed".into()))?
            .map_err(|e| {
                tracing::error!(error = %e, "ctranslate2 translation failed");
                TranslateError::Backend("ctranslate2 translation failed".into())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pg_models::{ModelFile, sha256_file};

    #[test]
    fn bundled_manifest_has_both_swedish_directions() {
        let manifest = Manifest::bundled().unwrap();
        let models: Vec<_> = manifest
            .models
            .iter()
            .filter(|m| m.engine == ENGINE)
            .collect();
        let pairs: Vec<_> = models.iter().map(|m| m.pair()).collect();
        assert!(pairs.contains(&LangPair::sv_en()) && pairs.contains(&LangPair::en_sv()));
        for m in &models {
            assert_eq!(m.license, "CC-BY-4.0", "{}", m.id);
            let attribution = m.attribution.as_deref().unwrap_or_default();
            assert!(attribution.contains("OPUS-MT"), "{}: {attribution}", m.id);
            for file in MODEL_FILES {
                assert!(m.files.iter().any(|f| f.name == file), "{}: {file}", m.id);
            }
        }
        let total: u64 = models
            .iter()
            .filter(|m| [LangPair::sv_en(), LangPair::en_sv()].contains(&m.pair()))
            .map(|m| m.download_size())
            .sum();
        // SIZE-05: both Swedish directions ≤ 200 MB.
        assert!(total <= 200_000_000, "{total}");
    }

    #[test]
    fn default_threads_are_capped() {
        let n = Options::default_threads();
        assert!((1..=MAX_DEFAULT_THREADS).contains(&n), "{n}");
        assert_eq!(Options::default().threads, n);
    }

    #[tokio::test]
    async fn without_models_every_pair_is_model_missing() {
        let t = Ct2Translator::load_installed(
            &Manifest::bundled().unwrap(),
            &ModelStore::new(tempfile::tempdir().unwrap().path()),
            Options::default(),
        )
        .unwrap();
        assert!(t.pairs().is_empty());
        assert!(!t.supports(&LangPair::sv_en()));
        assert!(t.is_local());
        assert_eq!(t.id(), ENGINE);
        assert_eq!(
            t.translate(&["Spara".into()], &LangPair::sv_en()).await,
            Err(TranslateError::ModelMissing(LangPair::sv_en()))
        );
    }

    fn fake_entry(src: &std::path::Path, names: &[&str]) -> ModelEntry {
        let files = names
            .iter()
            .map(|name| {
                let path = src.join(name);
                std::fs::write(&path, b"not a model").unwrap();
                ModelFile {
                    name: (*name).into(),
                    url: None,
                    size: 11,
                    sha256: sha256_file(&path).unwrap(),
                }
            })
            .collect();
        ModelEntry {
            id: "test-xx-yy".into(),
            engine: ENGINE.into(),
            source: pg_core::LangTag::sv(),
            target: pg_core::LangTag::en(),
            version: "1".into(),
            license: "MIT".into(),
            attribution: None,
            source_url: "https://example.org".into(),
            files,
        }
    }

    #[test]
    fn a_broken_installed_model_is_an_error_not_a_panic() {
        let src = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let entry = fake_entry(src.path(), &MODEL_FILES);
        let store = ModelStore::new(root.path());
        store.import_from_dir(&entry, src.path()).unwrap();
        let manifest = Manifest {
            schema_version: 1,
            models: vec![entry],
        };
        let err = Ct2Translator::load_installed(&manifest, &store, Options::default()).unwrap_err();
        assert!(matches!(err, LoadError::Engine { .. }), "{err}");
    }

    #[test]
    fn an_entry_without_the_model_files_is_rejected() {
        let src = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let entry = fake_entry(src.path(), &["model.bin", "config.json"]);
        let store = ModelStore::new(root.path());
        store.import_from_dir(&entry, src.path()).unwrap();
        let manifest = Manifest {
            schema_version: 1,
            models: vec![entry],
        };
        let err = Ct2Translator::load_installed(&manifest, &store, Options::default()).unwrap_err();
        assert!(
            matches!(
                err,
                LoadError::MissingFile {
                    file: "shared_vocabulary.json",
                    ..
                }
            ),
            "{err}"
        );
    }
}
