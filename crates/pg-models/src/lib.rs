//! On-device model management (MDL-01..06).
//!
//! Models download only from URLs in the shipped `models/manifest.toml` (MDL-02); every file
//! is verified against its SHA-256 before it is used.

mod download;
mod error;
mod manifest;
mod store;
mod verify;

pub use download::{Downloader, Progress};
pub use error::{ModelError, Result};
pub use manifest::{CatalogEntry, Manifest, ModelEntry, ModelFile};
pub use store::ModelStore;
pub use verify::{sha256_file, verify_file};
