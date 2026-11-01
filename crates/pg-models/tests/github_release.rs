//! MDL-02 / MDL-03 against the real GitHub release that hosts the OPUS-MT models (D-9, M1-TR-7).
//! They download from github.com, so they're ignored by default:
//!
//! ```text
//! cargo test -p pg-models --test github_release -- --ignored --nocapture
//! ```
//!
//! Only the small files are fetched (≈ 2 MB), not `model.bin`.

use pg_models::{Downloader, Manifest, ModelEntry, ModelStore};
use reqwest::{StatusCode, header, redirect};
use tokio_util::sync::CancellationToken;

/// The bundled sv→en entry, cut down to the named files.
fn sv_en_with(files: &[&str]) -> ModelEntry {
    let mut model = Manifest::bundled()
        .unwrap()
        .get("opus-mt-sv-en")
        .expect("opus-mt-sv-en in the manifest")
        .clone();
    model.files.retain(|f| files.contains(&f.name.as_str()));
    assert_eq!(model.files.len(), files.len());
    model
}

#[tokio::test]
#[ignore = "network: downloads from the GitHub release"]
async fn mdl02_release_redirects_to_an_https_asset_host() {
    let model = sv_en_with(&["config.json", "source.spm"]);
    let url = model.files[0].url.clone().unwrap();
    let client = reqwest::Client::builder()
        .redirect(redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client.get(&url).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::FOUND, "{url}");
    let location = resp.headers()[header::LOCATION].to_str().unwrap();
    let host = reqwest::Url::parse(location).unwrap();
    eprintln!(
        "{url}\n  -> {}://{}",
        host.scheme(),
        host.host_str().unwrap()
    );
    assert_eq!(host.scheme(), "https");
    assert_ne!(
        host.host_str(),
        Some("github.com"),
        "redirected to the asset host"
    );

    // The downloader follows it and verifies the SHA-256 from the manifest.
    let dir = tempfile::tempdir().unwrap();
    let store = ModelStore::new(dir.path());
    Downloader::new()
        .unwrap()
        .download(&store, &model, &CancellationToken::new(), |_| {})
        .await
        .unwrap();
    assert!(store.is_installed(&model));
}

#[tokio::test]
#[ignore = "network: downloads from the GitHub release"]
async fn mdl03_resume_with_range_through_the_release_redirect() {
    let model = sv_en_with(&["shared_vocabulary.json"]);
    let file = &model.files[0];
    let dl = Downloader::new().unwrap();

    // The whole file once, to cut a correct partial download from it.
    let full = tempfile::tempdir().unwrap();
    let full_store = ModelStore::new(full.path());
    dl.download(&full_store, &model, &CancellationToken::new(), |_| {})
        .await
        .unwrap();
    let bytes = std::fs::read(full_store.file_path(&model, &file.name)).unwrap();
    let have = bytes.len() / 2;

    // An interrupted download: the first half as `<name>.part`.
    let dir = tempfile::tempdir().unwrap();
    let store = ModelStore::new(dir.path());
    let part = store.model_dir(&model).join(format!("{}.part", file.name));
    std::fs::create_dir_all(store.model_dir(&model)).unwrap();
    std::fs::write(&part, &bytes[..have]).unwrap();

    let mut progress = Vec::new();
    dl.download(&store, &model, &CancellationToken::new(), |p| {
        progress.push(p.downloaded);
    })
    .await
    .unwrap();
    assert!(store.is_installed(&model));
    // A `206 Partial Content` keeps the first half; a `200` would restart from 0.
    eprintln!("resumed at {} of {} bytes", progress[0], file.size);
    assert_eq!(progress[0], have as u64, "{progress:?}");
    assert_eq!(*progress.last().unwrap(), file.size);
}
