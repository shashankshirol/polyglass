//! The translation backend under test: the mock, or the app's on-device backend (CTranslate2
//! with the installed OPUS-MT models behind the SV-05 token guard, as `live.rs` builds it).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use clap::ValueEnum;
use pg_core::LangPair;
use pg_ct2::{Ct2Translator, Options};
use pg_models::{Manifest, ModelStore};
use pg_translate::{MockTranslator, TokenGuard, Translator};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Backend {
    /// Returns `[target] text` instantly (pipeline overhead only, QUAL-02).
    Mock,
    /// CTranslate2 + OPUS-MT from the model store, as in the app (PERF-01, PERF-06, SV-07).
    Ct2,
}

/// Tauri's `app_local_data_dir()` for the app's identifier, plus `models`: where the app keeps
/// its models and `scripts/models/opus_mt.py` installs them.
pub fn app_model_store() -> Option<PathBuf> {
    const APP: &str = "org.polyglass.desktop";
    // Windows only (v1); elsewhere pass `--models`.
    let base = std::env::var_os("LOCALAPPDATA")
        .filter(|v| cfg!(windows) && !v.is_empty())
        .map(PathBuf::from)?;
    Some(base.join(APP).join("models"))
}

/// A loaded backend and how long loading took.
pub struct Loaded {
    pub translator: Arc<dyn Translator>,
    pub load_ms: f64,
    /// CPU threads per model (CTranslate2 only).
    pub threads: Option<usize>,
    /// The SV-05 guard around CTranslate2, for its counters; `None` with `--no-guard`.
    pub guard: Option<Arc<TokenGuard<Ct2Translator>>>,
}

/// SV-05 guard counters: blocks sent through its masked second pass, and blocks returned
/// untranslated because no pass kept their numbers, URLs and e-mails.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct GuardCounts {
    pub masked_retries: usize,
    pub fallbacks: usize,
}

impl Loaded {
    pub fn guard_counts(&self) -> Option<GuardCounts> {
        self.guard.as_ref().map(|g| GuardCounts {
            masked_retries: g.masked_retries(),
            fallbacks: g.fallbacks(),
        })
    }
}

/// Load `backend`; for `Ct2`, every installed model in `store` (default: the app's store), and
/// fail unless `pair` is among them. `guard: false` leaves out the SV-05 token guard.
pub fn load(
    backend: Backend,
    store: Option<PathBuf>,
    threads: Option<usize>,
    guard: bool,
    pair: &LangPair,
) -> Result<Loaded, String> {
    match backend {
        Backend::Mock => Ok(Loaded {
            translator: Arc::new(MockTranslator::new()),
            load_ms: 0.0,
            threads: None,
            guard: None,
        }),
        Backend::Ct2 => {
            let dir = store
                .or_else(app_model_store)
                .ok_or("no model store: pass --models <dir>")?;
            let manifest = Manifest::bundled().map_err(|e| e.to_string())?;
            let mut options = Options::default();
            if let Some(n) = threads {
                options.threads = n.max(1);
            }
            let start = Instant::now();
            let ct2 = Ct2Translator::load_installed(&manifest, &ModelStore::new(&dir), options)
                .map_err(|e| e.to_string())?;
            let load_ms = start.elapsed().as_secs_f64() * 1000.0;
            if !ct2.supports(pair) {
                return Err(format!(
                    "no {pair} model installed in {} (installed: {:?}); run `python scripts/models/opus_mt.py` or pass --models",
                    dir.display(),
                    ct2.pairs()
                ));
            }
            let threads = Some(ct2.options().threads);
            if !guard {
                return Ok(Loaded {
                    translator: Arc::new(ct2),
                    load_ms,
                    threads,
                    guard: None,
                });
            }
            let guarded = Arc::new(TokenGuard::new(ct2));
            Ok(Loaded {
                translator: guarded.clone(),
                load_ms,
                threads,
                guard: Some(guarded),
            })
        }
    }
}
