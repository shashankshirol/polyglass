//! Install models from local files into a model store (MDL-04).
//!
//! ```text
//! cargo run -p pg-models --bin import-models -- --from <dir> [--store <dir>] [<model id>...]
//! ```
//!
//! `<dir>` holds one sub-directory per model, named by its manifest id (`opus-mt-sv-en/`), as
//! `scripts/models/opus_mt.py` writes them. Every file is checked against the SHA-256 in the
//! bundled `models/manifest.toml`, exactly like a download. Without `--store` the models go to
//! the desktop app's store (see [`app_store`]). Without ids, every manifest model found in `<dir>`
//! is imported.

use std::path::PathBuf;
use std::process::ExitCode;

use pg_models::{Manifest, ModelStore};

/// The desktop app's model store: Tauri's `app_local_data_dir()` for the bundle identifier
/// `org.polyglass.desktop`, plus `models` (see `apps/desktop/src-tauri/src/main.rs`).
fn app_store() -> Option<PathBuf> {
    // Windows only (v1); elsewhere pass `--store`.
    let base = std::env::var_os("LOCALAPPDATA")
        .filter(|v| cfg!(windows) && !v.is_empty())
        .map(PathBuf::from)?;
    Some(base.join("org.polyglass.desktop").join("models"))
}

fn run() -> Result<(), String> {
    let mut from = None;
    let mut store = None;
    let mut ids = Vec::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--from") => from = args.next().map(PathBuf::from),
            Some("--store") => store = args.next().map(PathBuf::from),
            Some("-h" | "--help") => {
                return Err(
                    "usage: import-models --from <dir> [--store <dir>] [<model id>...]".into(),
                );
            }
            Some(id) if !id.starts_with('-') => ids.push(id.to_owned()),
            _ => return Err(format!("unknown argument {arg:?} (try --help)")),
        }
    }
    let from = from.ok_or("--from <dir> is required")?;
    let store = store
        .or_else(app_store)
        .ok_or("no --store given and the app's data directory is unknown")?;
    let manifest = Manifest::bundled().map_err(|e| e.to_string())?;
    let store = ModelStore::new(store);

    let models: Vec<_> = if ids.is_empty() {
        manifest
            .models
            .iter()
            .filter(|m| from.join(&m.id).is_dir())
            .collect()
    } else {
        ids.iter()
            .map(|id| manifest.get(id).ok_or(format!("{id}: not in the manifest")))
            .collect::<Result<_, _>>()?
    };
    if models.is_empty() {
        return Err(format!(
            "no model directories from the manifest in {}",
            from.display()
        ));
    }
    for m in models {
        store
            .import_from_dir(m, &from.join(&m.id))
            .map_err(|e| format!("{}: {e}", m.id))?;
        eprintln!(
            "installed {} ({}, {:.1} MB, {}) in {}",
            m.id,
            m.pair(),
            m.download_size() as f64 / 1e6,
            m.license,
            store.model_dir(m).display()
        );
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("import-models: {e}");
            ExitCode::FAILURE
        }
    }
}
