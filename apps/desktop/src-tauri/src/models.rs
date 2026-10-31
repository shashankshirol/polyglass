//! Swedish ↔ English model download from the lens toolbar (MDL-02, MDL-03, SV-01).
//!
//! Interim until the model manager and first-run wizard (M1-APP-8): when the models are missing,
//! the toolbar's ⬇ button downloads both directions from the URLs in `models/manifest.toml`
//! through `pg-models` (SHA-256 verified, resumable), then swaps the loaded translator into the
//! running pipeline. Nothing is downloaded without that click (MDL-06).
//!
//! The OPUS-MT entries are import-only until their files are hosted (D-9). Then there is no ⬇:
//! the lens points to `scripts/models/opus_mt.py`, which converts and imports them (MDL-04),
//! and the app loads them on its next start.

use std::sync::atomic::{AtomicBool, Ordering};

use pg_models::{Downloader, Manifest, ModelEntry, ModelStore};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

use crate::lens;
use crate::state::Lens;

/// Event carrying [`ModelEvent`]s to the lens.
pub const EVENT: &str = "lens-models";

/// Download state for the lens. Carries no text from the screen.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ModelEvent {
    Downloading { downloaded: u64, total: u64 },
    Ready,
    Error { message_id: &'static str },
}

#[derive(Debug)]
pub struct Models {
    store: ModelStore,
    manifest: Manifest,
    downloading: AtomicBool,
}

impl Models {
    pub fn new(store: ModelStore) -> Self {
        Self {
            store,
            // Validated by `pg_models` tests; an invalid bundled manifest is a build defect.
            manifest: Manifest::bundled().expect("bundled model manifest is valid"),
            downloading: AtomicBool::new(false),
        }
    }

    pub fn store(&self) -> &ModelStore {
        &self.store
    }

    fn missing(&self) -> Vec<ModelEntry> {
        self.manifest
            .swedish_english_models()
            .into_iter()
            .filter(|m| !self.store.is_installed(m))
            .cloned()
            .collect()
    }

    /// Bytes still to download for both Swedish directions (0 = installed).
    pub fn missing_bytes(&self) -> u64 {
        self.missing().iter().map(ModelEntry::download_size).sum()
    }

    /// Whether the missing models can be downloaded, i.e. the manifest has URLs for them.
    pub fn missing_downloadable(&self) -> bool {
        self.missing().iter().all(ModelEntry::is_downloadable)
    }
}

/// Start downloading the missing Swedish models unless a download is already running.
pub fn start_download(app: &AppHandle) {
    if app
        .state::<Models>()
        .downloading
        .swap(true, Ordering::SeqCst)
    {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let event = match download(&app).await {
            Ok(()) => ModelEvent::Ready,
            Err(error) => {
                // Technical context only (PRIV-07).
                tracing::error!(%error, "model download failed");
                ModelEvent::Error {
                    message_id: "mdl-error-download",
                }
            }
        };
        app.state::<Models>()
            .downloading
            .store(false, Ordering::SeqCst);
        let _ = app.emit_to(lens::LABEL, EVENT, event);
    });
}

async fn download(app: &AppHandle) -> Result<(), String> {
    let models = app.state::<Models>();
    let missing = models.missing();
    let total: u64 = missing.iter().map(ModelEntry::download_size).sum();
    let downloader = Downloader::new().map_err(|e| e.to_string())?;
    let cancel = CancellationToken::new();
    let mut done = 0;
    for model in &missing {
        tracing::info!(model = %model.id, bytes = model.download_size(), "downloading model");
        let mut last_percent = u64::MAX;
        downloader
            .download(models.store(), model, &cancel, |p| {
                let downloaded = done + p.downloaded;
                let percent = downloaded * 100 / total.max(1);
                if percent != last_percent {
                    last_percent = percent;
                    let _ = app.emit_to(
                        lens::LABEL,
                        EVENT,
                        ModelEvent::Downloading { downloaded, total },
                    );
                }
            })
            .await
            .map_err(|e| e.to_string())?;
        done += model.download_size();
    }
    let store = models.store().clone();
    let translator = tokio::task::spawn_blocking(move || crate::live::translator(&store))
        .await
        .map_err(|_| "loading the models panicked".to_owned())?;
    // If the pipeline isn't up yet it loads the installed models itself.
    app.state::<Lens>().set_translator(translator);
    Ok(())
}
