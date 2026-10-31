//! Resumable, cancellable model downloads with progress (MDL-03), verified against the manifest
//! SHA-256 before use (MDL-02).

use std::path::Path;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{StatusCode, header};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::{ModelEntry, ModelError, ModelStore, Result, verify_file};

/// Download progress for one model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Bytes on disk for this model so far (including resumed bytes).
    pub downloaded: u64,
    /// Total bytes for this model, from the manifest.
    pub total: u64,
}

#[derive(Debug, Clone)]
pub struct Downloader {
    client: reqwest::Client,
}

impl Downloader {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .user_agent(concat!("Polyglass/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| ModelError::Network(e.to_string()))?;
        Ok(Self { client })
    }

    /// Download every missing file of `model` into `store`.
    ///
    /// Interrupted downloads leave a `.part` file and resume from it with an HTTP Range request
    /// next time. A file that fails verification is deleted and reported (MDL-02). An
    /// import-only model (no URLs) is [`ModelError::NotHosted`].
    pub async fn download(
        &self,
        store: &ModelStore,
        model: &ModelEntry,
        cancel: &CancellationToken,
        mut on_progress: impl FnMut(Progress) + Send,
    ) -> Result<()> {
        if !model.is_downloadable() {
            return Err(ModelError::NotHosted(model.id.clone()));
        }
        fs::create_dir_all(store.model_dir(model)).await?;
        let total = model.download_size();
        let mut done_before = 0u64;
        for file in &model.files {
            let url = file.url.as_deref().ok_or(ModelError::InsecureUrl)?;
            check_url(url)?;
            let final_path = store.file_path(model, &file.name);
            if file_ok(&final_path, file.size, &file.sha256).await? {
                done_before += file.size;
                on_progress(Progress {
                    downloaded: done_before,
                    total,
                });
                continue;
            }
            let part = store.part_path(model, &file.name);
            self.fetch(url, &part, file.size, cancel, |n| {
                on_progress(Progress {
                    downloaded: done_before + n,
                    total,
                });
            })
            .await?;

            let part_c = part.clone();
            let sha = file.sha256.clone();
            let ok = tokio::task::spawn_blocking(move || verify_file(&part_c, &sha))
                .await
                .map_err(|e| ModelError::Network(e.to_string()))??;
            if !ok {
                let _ = fs::remove_file(&part).await;
                return Err(ModelError::HashMismatch {
                    file: file.name.clone(),
                });
            }
            fs::rename(&part, &final_path).await?;
            done_before += file.size;
        }
        store.mark_complete(model)?;
        Ok(())
    }

    /// Fetch `url` into `part`, resuming from its current length.
    async fn fetch(
        &self,
        url: &str,
        part: &Path,
        size: u64,
        cancel: &CancellationToken,
        mut on_bytes: impl FnMut(u64),
    ) -> Result<()> {
        let mut have = match fs::metadata(part).await {
            Ok(m) if m.len() <= size => m.len(),
            Ok(_) => {
                fs::remove_file(part).await?;
                0
            }
            Err(_) => 0,
        };
        if have == size {
            return Ok(());
        }
        let mut req = self.client.get(url);
        if have > 0 {
            req = req.header(header::RANGE, format!("bytes={have}-"));
        }
        let resp = tokio::select! {
            r = req.send() => r.map_err(|e| ModelError::Network(e.to_string()))?,
            () = cancel.cancelled() => return Err(ModelError::Cancelled),
        };
        let append = match resp.status() {
            StatusCode::PARTIAL_CONTENT if have > 0 => true,
            StatusCode::OK => {
                have = 0;
                false
            }
            s => return Err(ModelError::Http(s.as_u16())),
        };
        let mut out = OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(part)
            .await?;
        on_bytes(have);
        let mut stream = resp.bytes_stream();
        loop {
            let chunk = tokio::select! {
                c = stream.next() => c,
                () = cancel.cancelled() => {
                    out.flush().await?;
                    return Err(ModelError::Cancelled);
                }
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(|e| ModelError::Network(e.to_string()))?;
            have += chunk.len() as u64;
            if have > size {
                drop(out);
                let _ = fs::remove_file(part).await;
                return Err(ModelError::Network(
                    "server sent more data than expected".into(),
                ));
            }
            out.write_all(&chunk).await?;
            on_bytes(have);
        }
        out.flush().await?;
        if have < size {
            return Err(ModelError::Network(
                "connection closed before download finished".into(),
            ));
        }
        Ok(())
    }
}

async fn file_ok(path: &Path, size: u64, sha256: &str) -> Result<bool> {
    match fs::metadata(path).await {
        Ok(m) if m.len() == size => {
            let p = path.to_owned();
            let sha = sha256.to_owned();
            Ok(tokio::task::spawn_blocking(move || verify_file(&p, &sha))
                .await
                .map_err(|e| ModelError::Network(e.to_string()))??)
        }
        _ => Ok(false),
    }
}

/// HTTPS only (SEC-03); plain HTTP is accepted for loopback hosts so tests can use a local
/// server. Manifest validation separately requires https for every shipped URL.
fn check_url(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url).map_err(|_| ModelError::InsecureUrl)?;
    let loopback = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if loopback => Ok(()),
        _ => Err(ModelError::InsecureUrl),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_policy() {
        assert!(check_url("https://example.org/a").is_ok());
        assert!(check_url("http://127.0.0.1:8080/a").is_ok());
        assert!(check_url("http://example.org/a").is_err());
        assert!(check_url("ftp://example.org/a").is_err());
        assert!(check_url("not a url").is_err());
    }
}
