//! Tests against the real sv↔en models (M1-TR-1..4). They download about 30 MB from the URLs in
//! `models/manifest.toml` on first run, so they are `#[ignore]`d and never part of the CI test
//! path:
//!
//! ```text
//! cargo test -p pg-bergamot --release --test real_models -- --ignored --test-threads 1
//! ```
//!
//! Models go to `POLYGLASS_TEST_MODELS` if set, else to Cargo's target tmpdir, and are reused.

use std::io::Read;
use std::path::PathBuf;
use std::time::Instant;

use pg_bergamot::{BergamotTranslator, ModelFiles};
use pg_core::LangPair;
use pg_models::{Downloader, Manifest, ModelStore};
use pg_translate::tokens::check_preserved;
use pg_translate::{TokenGuard, Translator};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

/// Uncompressed SHA-256 of the model files, from Mozilla's model registry (`db/models.json`).
const UNCOMPRESSED_MODEL_SHA256: [(&str, &str); 2] = [
    (
        "bergamot-sv-en",
        "92d79e2725718b84aed486473bdfcbe6bba3e7094922d4bead8a121cfa346372",
    ),
    (
        "bergamot-en-sv",
        "65ea0db8e658435846f297384face62dbf1804cfc92de8388b0e368e6ecc25a7",
    ),
];

fn store() -> ModelStore {
    let root = std::env::var_os("POLYGLASS_TEST_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("models"));
    ModelStore::new(root)
}

/// Download (or resume, or just re-verify) both Swedish models through pg-models.
async fn ensure_models(manifest: &Manifest, store: &ModelStore) {
    let downloader = Downloader::new().unwrap();
    for m in manifest.swedish_english_models() {
        if !store.is_installed(m) {
            downloader
                .download(store, m, &CancellationToken::new(), |_| {})
                .await
                .unwrap_or_else(|e| panic!("download {}: {e}", m.id));
        }
    }
}

async fn translator() -> TokenGuard<BergamotTranslator> {
    let manifest = Manifest::bundled().unwrap();
    let store = store();
    ensure_models(&manifest, &store).await;
    TokenGuard::new(BergamotTranslator::load_installed(&manifest, &store).unwrap())
}

#[derive(serde::Deserialize)]
struct Fixture {
    blocks: Vec<Block>,
}
#[derive(serde::Deserialize)]
struct Block {
    text: String,
}

fn fixture(name: &str) -> Vec<String> {
    let path = format!(
        "{}/../../tests/fixtures/{name}/truth.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let fx: Fixture = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    fx.blocks.into_iter().map(|b| b.text).collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads ~30 MB of models"]
async fn mdl02_tr2_swedish_models_download_and_verify() {
    let manifest = Manifest::bundled().unwrap();
    let store = store();
    ensure_models(&manifest, &store).await;
    for m in manifest.swedish_english_models() {
        assert!(store.is_installed(m), "{}", m.id);
        // The manifest pins the .gz files; also check what's inside against Mozilla's registry.
        let files = ModelFiles::of(m).unwrap();
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(
            std::fs::File::open(store.file_path(m, &files.model)).unwrap(),
        )
        .read_to_end(&mut raw)
        .unwrap();
        let want = UNCOMPRESSED_MODEL_SHA256
            .iter()
            .find(|(id, _)| *id == m.id)
            .unwrap()
            .1;
        assert_eq!(hex::encode(Sha256::digest(&raw)), want, "{}", m.id);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the real models"]
async fn tr1_tr3_both_directions_loaded_and_translate() {
    let t = translator().await;
    assert_eq!(t.id(), "bergamot");
    assert!(t.is_local(), "TR-01: on-device");
    assert!(t.supports(&LangPair::sv_en()) && t.supports(&LangPair::en_sv()));

    // SV-01 / TR-3: both engines are already loaded, so swapping direction is just a call.
    for _ in 0..3 {
        let start = Instant::now();
        let en = t
            .translate(&["Spara ändringar?".into()], &LangPair::sv_en())
            .await
            .unwrap();
        let sv = t
            .translate(&["Save changes?".into()], &LangPair::en_sv())
            .await
            .unwrap();
        let both = start.elapsed();
        assert_eq!(en, ["Save changes?"]);
        assert!(sv[0].starts_with("Spara"), "{sv:?}");
        assert!(
            both.as_millis() < 500,
            "swap + 2 short strings took {both:?}"
        );
    }

    let dialog = fixture("ui-dialog-sv");
    let start = Instant::now();
    let out = t.translate(&dialog, &LangPair::sv_en()).await.unwrap();
    eprintln!(
        "ui-dialog-sv ({} blocks): {:?}",
        dialog.len(),
        start.elapsed()
    );
    assert_eq!(out.len(), dialog.len());
    assert_eq!(out.last().map(String::as_str), Some("Cancel"));
}

/// SV-05 on the real backend: every block of `numbers-sv` (sv→en) and `numbers-en` (en→sv)
/// keeps its digits, values, URLs and e-mail addresses.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the real models"]
async fn tr4_sv05_numbers_fixtures_keep_preserved_tokens() {
    let t = translator().await;
    for (name, pair) in [
        ("numbers-sv", LangPair::sv_en()),
        ("numbers-en", LangPair::en_sv()),
    ] {
        let blocks = fixture(name);
        let before = t.fallbacks();
        let out = t.translate(&blocks, &pair).await.unwrap();
        for (src, tr) in blocks.iter().zip(&out) {
            eprintln!("  {src} -> {tr}");
            assert_eq!(
                check_preserved(src, &pair.source, tr, &pair.target),
                Ok(()),
                "{src:?} -> {tr:?}"
            );
        }
        // The guard may show a block untranslated rather than wrong, but most must translate.
        let untranslated = blocks.iter().zip(&out).filter(|(s, t)| s == t).count();
        eprintln!(
            "{name}: masked retries so far {}, fallbacks {} (this fixture), untranslated {untranslated}/{}",
            t.masked_retries(),
            t.fallbacks() - before,
            blocks.len()
        );
        assert!(
            untranslated <= 2,
            "{name}: {untranslated} blocks untranslated: {out:?}"
        );
    }
}
