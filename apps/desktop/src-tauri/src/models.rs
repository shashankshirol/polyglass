//! The model manager and first-run wizard behind the `models` window (MDL-01, MDL-03, MDL-05,
//! MDL-06, SV-01).
//!
//! The window lists the manifest's catalog, Swedish pairs first, with size, license, attribution
//! and source (MDL-01). Downloads run one job at a time through `pg-models` (SHA-256 verified,
//! resumable with HTTP Range): the job reports progress, can be cancelled, and a cancelled or
//! interrupted download resumes from its `.part` files (MDL-03). Models can be deleted and the
//! store's disk usage is shown (MDL-05). After a download or a delete the running pipeline gets a
//! translator with the installed models, so no restart is needed.
//!
//! Nothing is downloaded without a click in this window (MDL-06). On the first live start
//! without the models of the selected pair (by default Swedish → English and its reverse) the
//! window opens as the wizard, with those directions pre-selected and their combined size shown
//! (SV-01); it is offered once (`[models] first_run_done` in `settings.toml`). Which models the
//! lens and the wizard offer follows the toolbar's language pair (`langs`, MDL-07).
//!
//! The OPUS-MT files are assets of a GitHub release of this repository (D-9, M1-TR-7). A model
//! without download URLs (import-only) can't be downloaded: the window shows the
//! `scripts/models/opus_mt.py` install command instead. "Import from folder" (MDL-04) installs
//! models from local files (the release's assets downloaded by hand, or what the script wrote),
//! verified against the manifest like a download, and loads them without a restart. The same happens when the release
//! answers HTTP 401/403/404/410 (e.g. while the repository is private): download buttons go away
//! for this run (`mdl-error-unreachable`). Other failures keep them for a retry.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use pg_core::LangPair;
use pg_models::{Downloader, Manifest, ModelEntry, ModelError, ModelStore};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tokio_util::sync::CancellationToken;

use crate::langs::Languages;
use crate::settings;
use crate::state::{Lens, Mode};

/// Label of the model manager window.
pub const WINDOW: &str = "models";
/// Event carrying a [`ModelsView`] to both windows whenever it changes.
pub const EVENT: &str = "models-changed";
/// How to install models without the release (MDL-04), run in a checkout of the repository.
/// With no model ids it installs the Swedish ↔ English pair (`DEFAULT_IDS` in the script).
pub const INSTALL_COMMAND: &str = "python scripts/models/opus_mt.py";
/// What the script installs when given no model ids.
const SCRIPT_DEFAULT_IDS: [&str; 2] = ["opus-mt-sv-en", "opus-mt-en-sv"];

/// At most this many ids per download request (SEC-01 input bound).
const MAX_IDS: usize = 32;
/// Longest id the manifest accepts.
const MAX_ID_LEN: usize = 128;

/// Message shown when the release isn't there for us (see the module docs).
const UNREACHABLE: &str = "mdl-error-unreachable";
/// Longest folder path the import takes from the webview (SEC-01 input bound).
const MAX_PATH_LEN: usize = 1024;

/// What "Import from folder" did (MDL-04).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportOutcome {
    /// Installed and verified.
    pub imported: Vec<String>,
    /// Found, but a file didn't match its SHA-256 (deleted) or couldn't be copied.
    pub failed: Vec<String>,
    /// Message ID when not everything went in: `mdl-import-none` (no model files there),
    /// `mdl-error-verify`, `mdl-error-disk`.
    pub message_id: Option<&'static str>,
}

/// Everything the model manager and the lens show about models. Carries no screen text.
#[derive(Debug, Clone, Serialize)]
pub struct ModelsView {
    /// MDL-01 catalog, Swedish pairs first.
    pub rows: Vec<ModelRow>,
    /// Bytes the model store uses, partial downloads included (MDL-05).
    pub disk_usage: u64,
    /// Where the models are stored.
    pub store_path: String,
    /// The running download, if any.
    pub job: Option<JobView>,
    /// Message ID of the last download or delete outcome worth telling (an error, or a stop).
    pub message_id: Option<&'static str>,
    /// The release answered that the files aren't there for us; downloads are off for this run.
    pub unreachable: bool,
    /// Whether the window opened as the first-run wizard (SV-01).
    pub first_run: bool,
    /// The install command for the models that can't be downloaded (or, if none, for the
    /// models of the selected pair).
    pub install_command: String,
    /// The selected pair's models, the selected direction first.
    pub wanted_ids: Vec<String>,
    /// Bytes of the selected pair's models still missing (0 = installed): the pair and its
    /// reverse if the catalog has one (`langs::Languages::wanted`).
    pub wanted_missing_bytes: u64,
    /// Whether those missing models can be downloaded (URLs and a reachable release).
    pub wanted_downloadable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelRow {
    pub id: String,
    pub source: String,
    pub target: String,
    pub version: String,
    pub download_size: u64,
    /// `false` for import-only entries (no download URLs).
    pub downloadable: bool,
    pub license: String,
    pub attribution: Option<String>,
    pub source_url: String,
    pub installed: bool,
    /// Bytes on disk for this model: the files when installed, else what a download resumes from.
    pub on_disk: u64,
    /// A model of the selected pair; pre-selected by the first-run wizard (SV-01).
    pub suggested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobView {
    pub ids: Vec<String>,
    /// Bytes on disk for the job's models so far, resumed bytes included.
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Debug)]
struct Job {
    view: JobView,
    cancel: CancellationToken,
    percent: u64,
}

#[derive(Debug)]
pub struct Models {
    store: ModelStore,
    manifest: Manifest,
    languages: Languages,
    /// The selected pair's models are offered (`set_wanted`).
    wanted: Mutex<Vec<LangPair>>,
    job: Mutex<Option<Job>>,
    message_id: Mutex<Option<&'static str>>,
    unreachable: AtomicBool,
    first_run: AtomicBool,
    /// An import runs (one at a time, and not during a download).
    importing: AtomicBool,
}

impl Models {
    pub fn new(store: ModelStore) -> Self {
        // Validated by `pg_models` tests; an invalid bundled manifest is a build defect.
        Self::with_manifest(
            store,
            Manifest::bundled().expect("bundled model manifest is valid"),
        )
    }

    pub fn with_manifest(store: ModelStore, manifest: Manifest) -> Self {
        let languages = Languages::from_manifest(&manifest);
        let wanted = languages.wanted(&languages.choice(&Default::default()));
        Self {
            store,
            manifest,
            languages,
            wanted: Mutex::new(wanted),
            job: Mutex::new(None),
            message_id: Mutex::new(None),
            unreachable: AtomicBool::new(false),
            first_run: AtomicBool::new(false),
            importing: AtomicBool::new(false),
        }
    }

    pub fn store(&self) -> &ModelStore {
        &self.store
    }

    pub fn languages(&self) -> &Languages {
        &self.languages
    }

    /// Offer the models of these pairs (the toolbar's pair and its reverse).
    pub fn set_wanted(&self, pairs: Vec<LangPair>) {
        *self.wanted.lock().expect("wanted lock") = pairs;
    }

    /// Whether a model for `pair` is installed.
    pub fn is_installed(&self, pair: &LangPair) -> bool {
        self.manifest
            .models
            .iter()
            .any(|m| m.pair() == *pair && self.store.is_installed(m))
    }

    /// The manifest entries for the wanted pairs, in that order.
    fn wanted_models(&self) -> Vec<&ModelEntry> {
        let wanted = self.wanted.lock().expect("wanted lock").clone();
        wanted
            .iter()
            .filter_map(|p| self.manifest.models.iter().find(|m| m.pair() == *p))
            .collect()
    }

    fn missing_wanted(&self) -> Vec<&ModelEntry> {
        self.wanted_models()
            .into_iter()
            .filter(|m| !self.store.is_installed(m))
            .collect()
    }

    /// Bytes still to download for the selected pair (0 = installed).
    pub fn missing_bytes(&self) -> u64 {
        self.missing_wanted()
            .iter()
            .map(|m| m.download_size())
            .sum()
    }

    /// Whether the missing models can be downloaded: the manifest has URLs for them and the
    /// release hasn't turned out to be unreachable.
    pub fn missing_downloadable(&self) -> bool {
        !self.unreachable.load(Ordering::SeqCst)
            && self.missing_wanted().iter().all(|m| m.is_downloadable())
    }

    /// The script command for the models that need it: not installed, and import-only or the
    /// release is unreachable; the wanted ones first. If none do, the wanted models'.
    pub fn install_command(&self) -> String {
        let unreachable = self.unreachable.load(Ordering::SeqCst);
        let wanted = self.wanted_models();
        let mut ids: Vec<&str> = wanted
            .iter()
            .copied()
            .chain(self.manifest.models.iter())
            .filter(|m| !self.store.is_installed(m) && (unreachable || !m.is_downloadable()))
            .map(|m| m.id.as_str())
            .collect();
        let mut seen = HashSet::new();
        ids.retain(|id| seen.insert(*id));
        if ids.is_empty() {
            ids = wanted.iter().map(|m| m.id.as_str()).collect();
        }
        install_command(&ids)
    }

    pub fn view(&self) -> ModelsView {
        let suggested: HashSet<String> =
            self.wanted_models().iter().map(|m| m.id.clone()).collect();
        let rows = self
            .manifest
            .catalog()
            .into_iter()
            .filter_map(|c| {
                let entry = self.manifest.get(&c.id)?;
                Some(ModelRow {
                    installed: self.store.is_installed(entry),
                    on_disk: self.store.model_disk_usage(entry).unwrap_or(0),
                    suggested: suggested.contains(&c.id),
                    version: entry.version.clone(),
                    source: c.pair.source.as_str().to_owned(),
                    target: c.pair.target.as_str().to_owned(),
                    id: c.id,
                    download_size: c.download_size,
                    downloadable: c.downloadable,
                    license: c.license,
                    attribution: c.attribution,
                    source_url: c.source_url,
                })
            })
            .collect();
        ModelsView {
            rows,
            disk_usage: self.store.disk_usage().unwrap_or(0),
            store_path: self.store.root().display().to_string(),
            job: self.lock_job().as_ref().map(|j| j.view.clone()),
            message_id: *self.message_id.lock().expect("message lock"),
            unreachable: self.unreachable.load(Ordering::SeqCst),
            first_run: self.first_run.load(Ordering::SeqCst),
            install_command: self.install_command(),
            wanted_ids: self.wanted_models().iter().map(|m| m.id.clone()).collect(),
            wanted_missing_bytes: self.missing_bytes(),
            wanted_downloadable: self.missing_downloadable(),
        }
    }

    fn lock_job(&self) -> std::sync::MutexGuard<'_, Option<Job>> {
        self.job.lock().expect("job lock")
    }

    fn set_message(&self, id: Option<&'static str>) {
        *self.message_id.lock().expect("message lock") = id;
    }

    /// The manifest entries for `ids` from the webview (SEC-01): 1..=32 known ids, duplicates
    /// dropped, manifest order kept.
    pub fn resolve(&self, ids: &[String]) -> Result<Vec<&ModelEntry>, String> {
        if ids.is_empty() || ids.len() > MAX_IDS {
            return Err(format!("expected 1 to {MAX_IDS} model ids"));
        }
        if let Some(bad) = ids
            .iter()
            .find(|id| id.len() > MAX_ID_LEN || self.manifest.get(id).is_none())
        {
            return Err(format!(
                "unknown model {:?}",
                bad.chars().take(MAX_ID_LEN).collect::<String>()
            ));
        }
        Ok(self
            .manifest
            .models
            .iter()
            .filter(|m| ids.contains(&m.id))
            .collect())
    }

    /// Start a job for the models in `ids` that aren't installed yet. `None` when all of them
    /// are; an error when the ids are invalid or another download runs.
    pub fn begin(
        &self,
        ids: &[String],
    ) -> Result<Option<(Vec<ModelEntry>, CancellationToken)>, String> {
        let entries: Vec<ModelEntry> = self
            .resolve(ids)?
            .into_iter()
            .filter(|m| !self.store.is_installed(m))
            .cloned()
            .collect();
        let mut job = self.lock_job();
        if job.is_some() || self.importing.load(Ordering::SeqCst) {
            return Err("a download or import is already running".into());
        }
        if entries.is_empty() {
            return Ok(None);
        }
        let cancel = CancellationToken::new();
        *job = Some(Job {
            view: JobView {
                ids: entries.iter().map(|m| m.id.clone()).collect(),
                downloaded: 0,
                total: entries.iter().map(ModelEntry::download_size).sum(),
            },
            cancel: cancel.clone(),
            percent: u64::MAX,
        });
        self.set_message(None);
        Ok(Some((entries, cancel)))
    }

    /// Record job progress; `true` when the whole percentage changed (worth an event).
    fn set_progress(&self, downloaded: u64) -> bool {
        let mut job = self.lock_job();
        let Some(job) = job.as_mut() else {
            return false;
        };
        job.view.downloaded = downloaded;
        let percent = downloaded * 100 / job.view.total.max(1);
        let changed = percent != job.percent;
        job.percent = percent;
        changed
    }

    /// Download `entries` one after another into the store, calling `notify` when the progress
    /// moved by a whole percent.
    pub async fn run_job(
        &self,
        entries: &[ModelEntry],
        cancel: &CancellationToken,
        mut notify: impl FnMut() + Send,
    ) -> Result<(), ModelError> {
        let downloader = Downloader::new()?;
        let mut done = 0;
        for model in entries {
            tracing::info!(model = %model.id, bytes = model.download_size(), "downloading model");
            downloader
                .download(&self.store, model, cancel, |p| {
                    if self.set_progress(done + p.downloaded) {
                        notify();
                    }
                })
                .await?;
            done += model.download_size();
        }
        Ok(())
    }

    /// End the running job with `result` and remember what to tell the user.
    pub fn finish(&self, result: &Result<(), ModelError>) {
        *self.lock_job() = None;
        let message = result.as_ref().err().map(error_message_id);
        if message == Some(UNREACHABLE) {
            self.unreachable.store(true, Ordering::SeqCst);
        }
        self.set_message(message);
    }

    /// Stop the running download; its `.part` files stay so the next download resumes (MDL-03).
    /// `false` when nothing was running.
    pub fn cancel(&self) -> bool {
        self.lock_job()
            .as_ref()
            .map(|j| j.cancel.cancel())
            .is_some()
    }

    /// The entry for `id` if it may be deleted now: known and not being downloaded.
    pub fn deletable(&self, id: &str) -> Result<ModelEntry, String> {
        let entry = self
            .resolve(std::slice::from_ref(&id.to_owned()))?
            .remove(0)
            .clone();
        if self
            .lock_job()
            .as_ref()
            .is_some_and(|j| j.view.ids.contains(&entry.id))
        {
            return Err("the model is being downloaded; cancel first".into());
        }
        Ok(entry)
    }

    /// Delete a model's files, partial downloads included (MDL-05).
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let entry = self.deletable(id)?;
        match self.store.delete(&entry) {
            Ok(()) => {
                tracing::info!(model = %entry.id, "model deleted");
                self.set_message(None);
                Ok(())
            }
            Err(error) => {
                tracing::error!(model = %entry.id, %error, "deleting the model failed");
                self.set_message(Some("mdl-error-delete"));
                Err(error.to_string())
            }
        }
    }

    /// Install every manifest model found in `dir` (MDL-04): release assets, the script's
    /// folders or a model's own folder (`ModelStore::import_found`), each file verified like a
    /// download. Refused while a download or another import runs.
    pub fn import(&self, dir: &Path) -> Result<ImportOutcome, String> {
        {
            let job = self.lock_job();
            if job.is_some() || self.importing.swap(true, Ordering::SeqCst) {
                return Err("a download or import is already running".into());
            }
        }
        let results = self.store.import_found(&self.manifest, dir);
        self.importing.store(false, Ordering::SeqCst);
        let mut out = ImportOutcome {
            imported: Vec::new(),
            failed: Vec::new(),
            message_id: results.is_empty().then_some("mdl-import-none"),
        };
        for (model, result) in results {
            match result {
                Ok(()) => {
                    tracing::info!(model = %model.id, "model imported");
                    out.imported.push(model.id.clone());
                }
                Err(error) => {
                    // Technical context only (PRIV-07); the path is the user's own folder.
                    tracing::warn!(model = %model.id, %error, "model import failed");
                    out.failed.push(model.id.clone());
                    out.message_id.get_or_insert(match error {
                        ModelError::HashMismatch { .. } => "mdl-error-verify",
                        _ => "mdl-error-disk",
                    });
                }
            }
        }
        self.set_message(None);
        Ok(out)
    }

    pub fn set_first_run(&self, on: bool) {
        self.first_run.store(on, Ordering::SeqCst);
    }

    fn take_first_run(&self) -> bool {
        self.first_run.swap(false, Ordering::SeqCst)
    }
}

/// The folder to import from, given by the webview (SEC-01): an absolute path of at most 1024
/// characters to an existing folder, or to a file in one (a dropped model file). Device paths
/// (`\\.\…`) are refused.
pub fn import_dir(path: &str) -> Result<PathBuf, String> {
    let path = path.trim();
    if path.is_empty() || path.chars().count() > MAX_PATH_LEN || path.contains('\0') {
        return Err("expected a folder path".into());
    }
    if path.starts_with(r"\\.\") {
        return Err("device paths can't be imported from".into());
    }
    let p = PathBuf::from(path);
    if !p.is_absolute() {
        return Err("the folder path must be absolute, e.g. C:\\Users\\me\\Downloads".into());
    }
    if p.is_dir() {
        Ok(p)
    } else if p.is_file()
        && let Some(parent) = p.parent()
    {
        Ok(parent.to_path_buf())
    } else {
        Err("no such folder".into())
    }
}

/// The script command that installs `ids`; the bare command installs the Swedish pair.
pub fn install_command(ids: &[&str]) -> String {
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    let mut default = SCRIPT_DEFAULT_IDS.to_vec();
    default.sort_unstable();
    if ids.is_empty() || sorted == default {
        INSTALL_COMMAND.to_owned()
    } else {
        format!("{INSTALL_COMMAND} {}", ids.join(" "))
    }
}

/// The wizard is offered on a live start while the selected pair's models are missing, once
/// (SV-01).
pub fn offer_first_run(mode: Mode, first_run_done: bool, missing_bytes: u64) -> bool {
    mode == Mode::Live && !first_run_done && missing_bytes > 0
}

/// The message for a failed download or a stopped one.
fn error_message_id(error: &ModelError) -> &'static str {
    match error {
        ModelError::Http(401 | 403 | 404 | 410) | ModelError::NotHosted(_) => UNREACHABLE,
        ModelError::Cancelled => "mdl-cancelled",
        ModelError::HashMismatch { .. } => "mdl-error-verify",
        ModelError::Io(_) => "mdl-error-disk",
        _ => "mdl-error-download",
    }
}

/// Tell both windows what changed.
pub fn emit(app: &AppHandle) {
    let _ = app.emit(EVENT, app.state::<Models>().view());
}

/// Open the model manager, or bring it to the front. Call it off the main thread's command
/// path (an `async` command, setup, or a menu handler): building a window inside a synchronous
/// command deadlocks on Windows.
pub fn open_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(WINDOW) {
        w.show()?;
        w.unminimize()?;
        // Models may have changed on disk since it last drew (e.g. installed by the script).
        emit(app);
        return w.set_focus();
    }
    let window = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("models.html".into()))
        .title("Polyglass – Translation models")
        .inner_size(640.0, 600.0)
        .min_inner_size(440.0, 380.0)
        .center()
        .additional_browser_args(crate::lens::WEBVIEW_ARGS)
        .build()?;
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            finish_first_run(&handle);
        }
    });
    window.set_focus()
}

/// The wizard was closed, declined or finished: don't offer it again (SV-01).
pub fn finish_first_run(app: &AppHandle) {
    if app.state::<Models>().take_first_run() {
        app.state::<settings::Store>()
            .update(|s| s.models.first_run_done = true);
        emit(app);
    }
}

/// Download the models in `ids` that are missing; progress arrives as [`EVENT`]s.
pub fn start_download(app: &AppHandle, ids: &[String]) -> Result<(), String> {
    let Some((entries, cancel)) = app.state::<Models>().begin(ids)? else {
        emit(app);
        return Ok(());
    };
    emit(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let models = app.state::<Models>();
        let result = models.run_job(&entries, &cancel, || emit(&app)).await;
        if let Err(error) = &result {
            // Technical context only (PRIV-07).
            tracing::warn!(%error, "model download stopped");
        }
        models.finish(&result);
        emit(&app);
        if result.is_ok() {
            reload_translator(&app).await;
        }
    });
    Ok(())
}

/// "Import from folder" (MDL-04): install what `path` holds and load it, without a restart.
pub async fn import(app: &AppHandle, path: &str) -> Result<ImportOutcome, String> {
    let dir = import_dir(path)?;
    let handle = app.clone();
    let out = tokio::task::spawn_blocking(move || handle.state::<Models>().import(&dir))
        .await
        .map_err(|_| "the import stopped unexpectedly".to_string())??;
    emit(app);
    if !out.imported.is_empty() {
        reload_translator(app).await;
    }
    Ok(out)
}

/// Delete a model and reload the translator without it.
pub fn delete(app: &AppHandle, id: &str) -> Result<(), String> {
    let result = app.state::<Models>().delete(id);
    emit(app);
    result?;
    let app = app.clone();
    tauri::async_runtime::spawn(async move { reload_translator(&app).await });
    Ok(())
}

/// Give the running live pipeline a translator with exactly the installed models, and redraw the
/// lens's pickers. If the pipeline isn't up yet it loads them itself; demo mode keeps its mock.
async fn reload_translator(app: &AppHandle) {
    // The pickers mark pairs whose model isn't installed (MDL-07).
    let _ = app.emit_to(
        crate::lens::LABEL,
        crate::langs::EVENT,
        crate::langs::view(app),
    );
    if app.state::<Lens>().mode != Mode::Live {
        return;
    }
    let store = app.state::<Models>().store().clone();
    match tokio::task::spawn_blocking(move || crate::live::translator(&store)).await {
        Ok(translator) => {
            app.state::<Lens>().set_translator(translator);
        }
        Err(_) => tracing::error!("loading the models panicked"),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::path::Path;

    use pg_models::sha256_file;

    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    /// A manifest with one import-only Swedish direction, one downloadable Swedish direction
    /// (`files`, served from `base`) and one German model.
    fn manifest(base: &str, files: &[(&str, &[u8])]) -> Manifest {
        let mut sha = Vec::new();
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in files {
            let p = dir.path().join(name);
            std::fs::write(&p, body).unwrap();
            sha.push(sha256_file(&p).unwrap());
        }
        let en_sv_files: Vec<String> = files
            .iter()
            .zip(&sha)
            .map(|((name, body), sha)| {
                format!(
                    r#"{{ name = "{name}", url = "https://example.org/en-sv/{name}", size = {}, sha256 = "{sha}" }}"#,
                    body.len()
                )
            })
            .collect();
        let any = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        let mut manifest = Manifest::parse(&format!(
            r#"
schema_version = 1

[[model]]
id = "t-de-en"
engine = "ctranslate2"
source = "de"
target = "en"
version = "1"
license = "MIT"
source_url = "https://example.org/de"
files = [{{ name = "model.bin", url = "https://example.org/de/model.bin", size = 4, sha256 = "{any}" }}]

[[model]]
id = "t-sv-en"
engine = "ctranslate2"
source = "sv"
target = "en"
version = "1"
license = "CC-BY-4.0"
attribution = "Example, CC-BY 4.0"
source_url = "https://example.org/sv"
files = [{{ name = "model.bin", size = 4, sha256 = "{any}" }}]

[[model]]
id = "t-en-sv"
engine = "ctranslate2"
source = "en"
target = "sv"
version = "2"
license = "MIT"
source_url = "https://example.org/sv"
files = [{}]
"#,
            en_sv_files.join(", ")
        ))
        .unwrap();
        // The manifest takes https only; the downloader also takes loopback http (tests).
        for file in &mut manifest.models[2].files {
            file.url = Some(format!("{base}/{}", file.name));
        }
        manifest
    }

    fn models(root: &Path, base: &str, files: &[(&str, &[u8])]) -> Models {
        Models::with_manifest(ModelStore::new(root), manifest(base, files))
    }

    #[test]
    fn mdl01_rows_list_swedish_first_with_size_license_and_source() {
        let dir = tempfile::tempdir().unwrap();
        let m = models(
            dir.path(),
            "https://example.org/en-sv",
            &[("model.bin", b"abc")],
        );
        let view = m.view();
        let order: Vec<_> = view.rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(order, ["t-en-sv", "t-sv-en", "t-de-en"]);
        let sv_en = &view.rows[1];
        assert_eq!((sv_en.source.as_str(), sv_en.target.as_str()), ("sv", "en"));
        assert_eq!(sv_en.license, "CC-BY-4.0");
        assert_eq!(sv_en.attribution.as_deref(), Some("Example, CC-BY 4.0"));
        assert_eq!(sv_en.source_url, "https://example.org/sv");
        assert!(!sv_en.downloadable, "import-only");
        assert_eq!(view.rows[0].download_size, 3);
        assert_eq!(view.rows[0].version, "2");
        // SV-01: the wizard pre-selects both Swedish directions and nothing else.
        let suggested: Vec<_> = view.rows.iter().map(|r| r.suggested).collect();
        assert_eq!(suggested, [true, true, false]);
        assert_eq!(
            view.wanted_ids,
            ["t-sv-en", "t-en-sv"],
            "selected direction first"
        );
        assert_eq!(view.wanted_missing_bytes, 7);
        assert!(!view.wanted_downloadable, "one direction is import-only");
        assert!(view.rows.iter().all(|r| !r.installed && r.on_disk == 0));
        assert_eq!(
            (view.disk_usage, view.job, view.message_id),
            (0, None, None)
        );
        // The import-only Swedish direction needs the script: just that model.
        assert_eq!(view.install_command, format!("{INSTALL_COMMAND} t-sv-en"));
    }

    #[test]
    fn mdl07_the_selected_pair_decides_what_is_offered() {
        let dir = tempfile::tempdir().unwrap();
        let m = models(dir.path(), "https://example.org", &[("model.bin", b"abc")]);
        let de_en = LangPair::new(
            pg_core::LangTag::parse("de").unwrap(),
            pg_core::LangTag::en(),
        );
        m.set_wanted(vec![de_en.clone()]);
        let view = m.view();
        let suggested: Vec<_> = view.rows.iter().map(|r| r.suggested).collect();
        assert_eq!(suggested, [false, false, true], "only German → English");
        assert_eq!(view.wanted_missing_bytes, 4);
        assert!(view.wanted_downloadable);
        assert!(!m.is_installed(&de_en));
        // The release turned out unreachable: every missing model needs the script, the
        // selected pair's first.
        m.finish(&Err(ModelError::Http(404)));
        assert_eq!(
            m.install_command(),
            format!("{INSTALL_COMMAND} t-de-en t-sv-en t-en-sv")
        );
    }

    #[test]
    fn mdl04_import_folder_path_is_checked() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t-sv-en.model.bin");
        std::fs::write(&file, b"test").unwrap();
        assert_eq!(
            import_dir(dir.path().to_str().unwrap()).unwrap(),
            dir.path()
        );
        // A dropped file stands for its folder.
        assert_eq!(import_dir(file.to_str().unwrap()).unwrap(), dir.path());
        let long = format!("C:/{}", "a".repeat(MAX_PATH_LEN));
        for bad in [
            "",
            "   ",
            "models",
            "..",
            long.as_str(),
            "C:/no/such/folder/polyglass",
            r"\\.\PhysicalDrive0",
            "C:/a\0b",
        ] {
            assert!(import_dir(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn mdl04_import_from_a_folder_installs_verifies_and_reports() {
        let root = tempfile::tempdir().unwrap();
        let m = models(root.path(), "https://example.org", &[("model.bin", b"abc")]);
        let src = tempfile::tempdir().unwrap();
        // Nothing from the manifest there.
        std::fs::write(src.path().join("notes.txt"), b"test").unwrap();
        let out = m.import(src.path()).unwrap();
        assert_eq!(
            (out.imported.len(), out.failed.len(), out.message_id),
            (0, 0, Some("mdl-import-none"))
        );
        // A release asset whose bytes are wrong: deleted, reported, nothing installed.
        std::fs::write(src.path().join("t-sv-en.model.bin"), b"evil").unwrap();
        let out = m.import(src.path()).unwrap();
        assert_eq!(out.failed, ["t-sv-en"]);
        assert_eq!(out.message_id, Some("mdl-error-verify"));
        assert!(!m.view().rows.iter().any(|r| r.installed));
        // The right bytes: installed, the import-only model needs no download any more.
        std::fs::write(src.path().join("t-sv-en.model.bin"), b"test").unwrap();
        let out = m.import(src.path()).unwrap();
        assert_eq!(
            (out.imported, out.message_id),
            (vec!["t-sv-en".to_string()], None)
        );
        let row = m
            .view()
            .rows
            .into_iter()
            .find(|r| r.id == "t-sv-en")
            .unwrap();
        assert!(row.installed);
        // Not while a download runs.
        let _job = m.begin(&ids(&["t-en-sv"])).unwrap().unwrap();
        assert!(m.import(src.path()).is_err());
    }

    #[test]
    fn mdl04_install_command_names_models_unless_it_is_the_default_pair() {
        assert_eq!(install_command(&[]), INSTALL_COMMAND);
        assert_eq!(
            install_command(&["opus-mt-en-sv", "opus-mt-sv-en"]),
            INSTALL_COMMAND
        );
        assert_eq!(
            install_command(&["opus-mt-de-en"]),
            "python scripts/models/opus_mt.py opus-mt-de-en"
        );
        assert_eq!(
            install_command(&["opus-mt-sv-en"]),
            "python scripts/models/opus_mt.py opus-mt-sv-en"
        );
    }

    #[test]
    fn sec01_download_and_delete_take_only_known_ids() {
        let dir = tempfile::tempdir().unwrap();
        let m = models(dir.path(), "https://example.org", &[("model.bin", b"abc")]);
        for bad in [
            ids(&[]),
            ids(&["nope"]),
            ids(&["t-sv-en", "../t-sv-en"]),
            vec!["x".repeat(10_000)],
            vec!["t-sv-en".to_owned(); MAX_IDS + 1],
        ] {
            assert!(m.resolve(&bad).is_err(), "{:?}", bad.first());
        }
        let resolved = m.resolve(&ids(&["t-en-sv", "t-de-en", "t-en-sv"])).unwrap();
        let got: Vec<_> = resolved.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(got, ["t-de-en", "t-en-sv"], "deduplicated, manifest order");
        assert!(m.deletable("t-de-en").is_ok());
        assert!(m.deletable("").is_err());
        assert!(m.deletable("t-de-en/..").is_err());
        assert!(m.delete("nope").is_err());
    }

    #[test]
    fn mdl03_one_job_at_a_time_cancel_and_no_delete_while_downloading() {
        let dir = tempfile::tempdir().unwrap();
        let m = models(dir.path(), "https://example.org", &[("model.bin", b"abc")]);
        assert!(!m.cancel(), "nothing to cancel");
        let (entries, cancel) = m.begin(&ids(&["t-en-sv", "t-de-en"])).unwrap().unwrap();
        assert_eq!(entries.len(), 2);
        let job = m.view().job.unwrap();
        assert_eq!((job.downloaded, job.total), (0, 7));
        assert!(m.begin(&ids(&["t-sv-en"])).is_err(), "second job refused");
        assert!(m.deletable("t-en-sv").is_err(), "being downloaded");
        assert!(m.deletable("t-sv-en").is_ok(), "not in the job");
        assert!(m.set_progress(3));
        assert!(!m.set_progress(3), "same percent: no event");
        assert_eq!(m.view().job.unwrap().downloaded, 3);
        assert!(m.cancel());
        assert!(cancel.is_cancelled());
        m.finish(&Err(ModelError::Cancelled));
        let view = m.view();
        assert_eq!((view.job, view.message_id), (None, Some("mdl-cancelled")));
        assert!(!view.unreachable);
        assert!(m.begin(&ids(&["t-sv-en"])).unwrap().is_some(), "free again");
    }

    #[test]
    fn mdl03_unreachable_release_falls_back_to_the_install_command() {
        for status in [401, 403, 404, 410] {
            assert_eq!(error_message_id(&ModelError::Http(status)), UNREACHABLE);
        }
        assert_eq!(
            error_message_id(&ModelError::NotHosted("opus-mt-sv-en".into())),
            UNREACHABLE
        );
        for (error, id) in [
            (ModelError::Http(500), "mdl-error-download"),
            (ModelError::Http(429), "mdl-error-download"),
            (
                ModelError::Network("timed out".into()),
                "mdl-error-download",
            ),
            (ModelError::Cancelled, "mdl-cancelled"),
            (
                ModelError::HashMismatch {
                    file: "model.bin".into(),
                },
                "mdl-error-verify",
            ),
            (
                ModelError::Io(std::io::Error::other("full")),
                "mdl-error-disk",
            ),
        ] {
            assert_eq!(error_message_id(&error), id, "{error}");
        }

        // An unreachable answer turns downloads off for the run; the view says so.
        let dir = tempfile::tempdir().unwrap();
        let m = models(dir.path(), "https://example.org", &[("model.bin", b"abc")]);
        m.begin(&ids(&["t-en-sv"])).unwrap();
        m.finish(&Err(ModelError::Http(404)));
        let view = m.view();
        assert!(view.unreachable && !view.wanted_downloadable);
        assert_eq!(view.message_id, Some(UNREACHABLE));
        assert!(!m.missing_downloadable());
    }

    #[tokio::test]
    async fn mdl06_import_only_model_downloads_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let m = models(dir.path(), "https://example.org", &[("model.bin", b"abc")]);
        let (entries, cancel) = m.begin(&ids(&["t-sv-en"])).unwrap().unwrap();
        let result = m.run_job(&entries, &cancel, || {}).await;
        assert!(
            matches!(result, Err(ModelError::NotHosted(_))),
            "{result:?}"
        );
        m.finish(&result);
        assert_eq!(m.view().message_id, Some(UNREACHABLE));
        assert_eq!(m.view().disk_usage, 0);
    }

    #[tokio::test]
    async fn mdl03_mdl05_download_resume_then_delete() {
        const MODEL: &[u8] = b"model bytes for the en-sv test model";
        const VOCAB: &[u8] = b"vocab";
        let (base, seen) = serve_files(&[("model.bin", MODEL), ("vocab.spm", VOCAB)]);
        let dir = tempfile::tempdir().unwrap();
        let m = models(
            dir.path(),
            &base,
            &[("model.bin", MODEL), ("vocab.spm", VOCAB)],
        );
        let entry = m.manifest.get("t-en-sv").unwrap().clone();

        // An interrupted download left the first 10 bytes; the job resumes from them.
        let model_dir = m.store().model_dir(&entry);
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.bin.part"), &MODEL[..10]).unwrap();
        let resumable = m.view();
        assert_eq!(resumable.rows[0].on_disk, 10);
        assert!(!resumable.rows[0].installed);

        let (entries, cancel) = m.begin(&ids(&["t-en-sv"])).unwrap().unwrap();
        let mut events = 0;
        let result = m.run_job(&entries, &cancel, || events += 1).await;
        result.as_ref().unwrap();
        m.finish(&result);
        assert!(events >= 2, "progress reported ({events})");
        assert_eq!(
            *seen.lock().unwrap(),
            [("/model.bin".to_owned(), 10), ("/vocab.spm".to_owned(), 0)],
            "resumed at byte 10"
        );

        let view = m.view();
        let row = &view.rows[0];
        assert!(row.installed);
        assert_eq!(
            row.on_disk,
            (MODEL.len() + VOCAB.len() + 1) as u64,
            "+ marker"
        );
        assert_eq!(view.disk_usage, row.on_disk);
        assert_eq!((view.job, view.message_id), (None, None));
        assert_eq!(std::fs::read(model_dir.join("model.bin")).unwrap(), MODEL);
        assert!(m.begin(&ids(&["t-en-sv"])).unwrap().is_none(), "installed");

        m.delete("t-en-sv").unwrap();
        let view = m.view();
        assert!(!view.rows[0].installed);
        assert_eq!(view.disk_usage, 0);
    }

    /// Requests a test server saw: path and the first byte asked for.
    type Seen = std::sync::Arc<Mutex<Vec<(String, usize)>>>;

    /// A loopback HTTP server for one request per file: `<base>/<name>` serves that file and
    /// honours `Range: bytes=N-`. Returns the base URL and the requests it saw.
    fn serve_files(files: &[(&'static str, &'static [u8])]) -> (String, Seen) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let files: Vec<(&'static str, &'static [u8])> = files.to_vec();
        let seen = Seen::default();
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(files.len()) {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let body = files
                    .iter()
                    .find(|(name, _)| path.ends_with(name))
                    .map_or(&b""[..], |(_, body)| *body);
                let mut from = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(r) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        from = r.trim().trim_end_matches('-').parse().unwrap();
                    }
                }
                log.lock().unwrap().push((path, from));
                let part = &body[from..];
                let status = if from > 0 {
                    "206 Partial Content"
                } else {
                    "200 OK"
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    part.len()
                );
                let _ = stream.write_all(part);
                let _ = stream.read(&mut [0; 1]);
            }
        });
        (base, seen)
    }

    #[test]
    fn sv01_wizard_is_offered_once_on_a_live_start_without_the_models() {
        assert!(offer_first_run(Mode::Live, false, 1));
        assert!(!offer_first_run(Mode::Live, true, 1), "offered before");
        assert!(!offer_first_run(Mode::Live, false, 0), "models installed");
        assert!(!offer_first_run(Mode::Demo, false, 1), "demo mode");
        assert!(!offer_first_run(Mode::None, false, 1), "no pipeline");
    }
}
