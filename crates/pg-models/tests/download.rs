//! MDL-02 / MDL-03 download tests against a tiny local HTTP server.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pg_core::LangTag;
use pg_models::{Downloader, ModelEntry, ModelError, ModelFile, ModelStore, Progress};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct ServerState {
    files: HashMap<String, Vec<u8>>,
    /// Close the connection after this many body bytes, once per path.
    cut_after: HashMap<String, usize>,
    /// Stall forever after this many body bytes.
    stall_after: HashMap<String, usize>,
    /// Answer these paths with `302 Found` to the given location.
    redirects: HashMap<String, String>,
    /// Range header of each request, in order (`None` = no Range).
    ranges: Vec<Option<String>>,
}

type State = Arc<Mutex<ServerState>>;

async fn serve(state: State) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let state = state.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = sock.read(&mut tmp).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                let req = String::from_utf8_lossy(&buf).to_string();
                let path = req.split_whitespace().nth(1).unwrap().to_string();
                let range = req
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                    .map(|l| l[6..].trim().to_string());
                let (body, cut, stall, redirect) = {
                    let mut s = state.lock().unwrap();
                    s.ranges.push(range.clone());
                    let cut = s.cut_after.remove(&path);
                    let stall = s.stall_after.get(&path).copied();
                    let redirect = s.redirects.get(&path).cloned();
                    (s.files.get(&path).cloned(), cut, stall, redirect)
                };
                if let Some(location) = redirect {
                    let head = format!(
                        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.shutdown().await;
                    return;
                }
                let Some(body) = body else {
                    let _ = sock
                        .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                        .await;
                    return;
                };
                let start = range
                    .as_deref()
                    .and_then(|r| r.strip_prefix("bytes="))
                    .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok())
                    .unwrap_or(0);
                let slice = &body[start..];
                let head = if start > 0 {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        slice.len(),
                        start,
                        body.len() - 1,
                        body.len()
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        slice.len()
                    )
                };
                sock.write_all(head.as_bytes()).await.unwrap();
                if let Some(n) = stall {
                    let _ = sock.write_all(&slice[..n.min(slice.len())]).await;
                    let _ = sock.flush().await;
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                }
                let n = cut.unwrap_or(slice.len()).min(slice.len());
                let _ = sock.write_all(&slice[..n]).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    format!("http://{addr}")
}

fn sha(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn entry(base: &str, files: &[(&str, &[u8])]) -> ModelEntry {
    ModelEntry {
        id: "test-sv-en".into(),
        engine: "ctranslate2".into(),
        source: LangTag::sv(),
        target: LangTag::en(),
        version: "1".into(),
        license: "MIT".into(),
        attribution: None,
        source_url: "https://example.org".into(),
        files: files
            .iter()
            .map(|(name, data)| ModelFile {
                name: (*name).into(),
                url: Some(format!("{base}/{name}")),
                size: data.len() as u64,
                sha256: sha(data),
            })
            .collect(),
    }
}

fn payload(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

async fn setup(files: &[(&str, Vec<u8>)]) -> (State, String) {
    let state: State = Arc::default();
    {
        let mut s = state.lock().unwrap();
        for (name, data) in files {
            s.files.insert(format!("/{name}"), data.clone());
        }
    }
    let base = serve(state.clone()).await;
    (state, base)
}

#[tokio::test]
async fn downloads_verifies_and_installs() {
    let a = payload(200_000, 1);
    let b = payload(1_000, 2);
    let (state, base) = setup(&[("model.bin", a.clone()), ("vocab.spm", b.clone())]).await;
    let model = entry(&base, &[("model.bin", &a), ("vocab.spm", &b)]);
    let dir = tempfile::tempdir().unwrap();
    let store = ModelStore::new(dir.path());
    let mut last = Progress {
        downloaded: 0,
        total: 0,
    };
    Downloader::new()
        .unwrap()
        .download(&store, &model, &CancellationToken::new(), |p| last = p)
        .await
        .unwrap();
    assert!(store.is_installed(&model));
    assert_eq!(last.downloaded, last.total);
    assert_eq!(last.total, 201_000);
    assert_eq!(
        std::fs::read(store.file_path(&model, "model.bin")).unwrap(),
        a
    );

    // Already installed: verified locally, no new requests.
    let before = state.lock().unwrap().ranges.len();
    Downloader::new()
        .unwrap()
        .download(&store, &model, &CancellationToken::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(state.lock().unwrap().ranges.len(), before);
}

#[tokio::test]
async fn mdl03_resumes_after_interruption() {
    let a = payload(100_000, 3);
    let (state, base) = setup(&[("model.bin", a.clone())]).await;
    state
        .lock()
        .unwrap()
        .cut_after
        .insert("/model.bin".into(), 40_000);
    let model = entry(&base, &[("model.bin", &a)]);
    let dir = tempfile::tempdir().unwrap();
    let store = ModelStore::new(dir.path());
    let dl = Downloader::new().unwrap();

    let err = dl
        .download(&store, &model, &CancellationToken::new(), |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, ModelError::Network(_)), "{err:?}");
    assert!(!store.is_installed(&model));

    dl.download(&store, &model, &CancellationToken::new(), |_| {})
        .await
        .unwrap();
    assert!(store.is_installed(&model));
    assert_eq!(
        std::fs::read(store.file_path(&model, "model.bin")).unwrap(),
        a
    );
    let ranges = state.lock().unwrap().ranges.clone();
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0], None);
    let resumed_from: u64 = ranges[1]
        .as_deref()
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.trim_end_matches('-').parse().ok())
        .expect("second request is a Range request");
    assert!(
        resumed_from > 0 && resumed_from <= 40_000,
        "resumed from {resumed_from}"
    );
}

/// GitHub release downloads answer with a redirect to the asset host; the Range header of a
/// resumed download must reach the host that serves the file.
#[tokio::test]
async fn mdl03_follows_redirects_and_resumes_through_them() {
    let a = payload(100_000, 8);
    let (state, base) = setup(&[("assets/model.bin", a.clone())]).await;
    {
        let mut s = state.lock().unwrap();
        s.redirects
            .insert("/model.bin".into(), format!("{base}/assets/model.bin"));
        s.cut_after.insert("/assets/model.bin".into(), 40_000);
    }
    let model = entry(&base, &[("model.bin", &a)]);
    let dir = tempfile::tempdir().unwrap();
    let store = ModelStore::new(dir.path());
    let dl = Downloader::new().unwrap();

    let err = dl
        .download(&store, &model, &CancellationToken::new(), |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, ModelError::Network(_)), "{err:?}");
    dl.download(&store, &model, &CancellationToken::new(), |_| {})
        .await
        .unwrap();
    assert!(store.is_installed(&model));
    assert_eq!(
        std::fs::read(store.file_path(&model, "model.bin")).unwrap(),
        a
    );
    // Redirect + file, then the same two requests resumed with a Range.
    let ranges = state.lock().unwrap().ranges.clone();
    assert_eq!(ranges.len(), 4, "{ranges:?}");
    assert_eq!(ranges[..2], [None, None]);
    assert!(
        ranges[3].is_some(),
        "the asset host got the Range: {ranges:?}"
    );
    assert_eq!(ranges[2], ranges[3]);
}

#[tokio::test]
async fn sec03_redirect_to_plain_http_is_refused() {
    let a = payload(1_000, 9);
    let (state, base) = setup(&[]).await;
    state.lock().unwrap().redirects.insert(
        "/model.bin".into(),
        "http://example.invalid/model.bin".into(),
    );
    let model = entry(&base, &[("model.bin", &a)]);
    let dir = tempfile::tempdir().unwrap();
    let store = ModelStore::new(dir.path());
    let err = Downloader::new()
        .unwrap()
        .download(&store, &model, &CancellationToken::new(), |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, ModelError::InsecureUrl), "{err:?}");
    assert!(!store.is_installed(&model));
    assert_eq!(state.lock().unwrap().ranges.len(), 1);
}

#[tokio::test]
async fn mdl02_hash_mismatch_deletes_file() {
    let real = payload(5_000, 4);
    let tampered = payload(5_000, 5);
    let (_state, base) = setup(&[("model.bin", tampered)]).await;
    let model = entry(&base, &[("model.bin", &real)]);
    let dir = tempfile::tempdir().unwrap();
    let store = ModelStore::new(dir.path());
    let err = Downloader::new()
        .unwrap()
        .download(&store, &model, &CancellationToken::new(), |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, ModelError::HashMismatch { ref file } if file == "model.bin"));
    assert!(!store.is_installed(&model));
    assert!(!store.file_path(&model, "model.bin").exists());
    assert!(!dir.path().join("test-sv-en/model.bin.part").exists());
}

#[tokio::test]
async fn mdl03_cancel_keeps_partial_file() {
    let a = payload(100_000, 6);
    let (state, base) = setup(&[("model.bin", a.clone())]).await;
    state
        .lock()
        .unwrap()
        .stall_after
        .insert("/model.bin".into(), 10_000);
    let model = entry(&base, &[("model.bin", &a)]);
    let dir = tempfile::tempdir().unwrap();
    let store = ModelStore::new(dir.path());
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    let err = Downloader::new()
        .unwrap()
        .download(&store, &model, &cancel, move |p| {
            if p.downloaded >= 10_000 {
                c2.cancel();
            }
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ModelError::Cancelled), "{err:?}");
    let part = dir.path().join("test-sv-en/model.bin.part");
    assert_eq!(std::fs::metadata(part).unwrap().len(), 10_000);
}

#[tokio::test]
async fn http_errors_are_reported() {
    let (_state, base) = setup(&[]).await;
    let model = entry(&base, &[("missing.bin", b"x")]);
    let dir = tempfile::tempdir().unwrap();
    let err = Downloader::new()
        .unwrap()
        .download(
            &ModelStore::new(dir.path()),
            &model,
            &CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ModelError::Http(404)));
}

#[tokio::test]
async fn mdl04_import_only_model_is_not_downloaded() {
    let a = payload(1_000, 7);
    let mut model = entry("https://example.invalid", &[("model.bin", &a)]);
    for f in &mut model.files {
        f.url = None;
    }
    assert!(!model.is_downloadable());
    let dir = tempfile::tempdir().unwrap();
    let store = ModelStore::new(dir.path());
    let err = Downloader::new()
        .unwrap()
        .download(&store, &model, &CancellationToken::new(), |_| {})
        .await
        .unwrap_err();
    assert!(
        matches!(err, ModelError::NotHosted(ref id) if id == "test-sv-en"),
        "{err}"
    );
    assert!(!store.model_dir(&model).exists(), "nothing written");

    // It installs by import instead, verified the same way.
    let src = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("model.bin"), &a).unwrap();
    store.import_from_dir(&model, src.path()).unwrap();
    assert!(store.is_installed(&model));
}
