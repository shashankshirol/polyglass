//! Tests against the real OPUS-MT sv↔en models (M1-TR-1..4) and de→en (M3-9). They need the output of
//! `scripts/models/opus_mt.py` (the same files as the GitHub release, D-9) and are `#[ignore]`d:
//!
//! ```text
//! python scripts/models/opus_mt.py --no-install
//! cargo test -p pg-ct2 --release --test real_models -- --ignored --test-threads 1 --nocapture
//! ```
//!
//! The converted models are imported (SHA-256 checked against the manifest) from
//! `POLYGLASS_OPUS_MT_DIR`, default `target/opus-mt/out`, into the store at
//! `POLYGLASS_TEST_MODELS`, default Cargo's target tmpdir, and reused from there.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use pg_core::LangPair;
use pg_ct2::{Ct2Translator, Options};
use pg_models::{Manifest, ModelEntry, ModelStore};
use pg_translate::guard::{mask, unmask};
use pg_translate::tokens::check_preserved;
use pg_translate::{TokenGuard, Translator};

fn store() -> ModelStore {
    let root = std::env::var_os("POLYGLASS_TEST_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("models"));
    ModelStore::new(root)
}

fn swedish_models(manifest: &Manifest) -> Vec<&ModelEntry> {
    manifest
        .models
        .iter()
        .filter(|m| m.engine == pg_ct2::ENGINE)
        .filter(|m| [LangPair::sv_en(), LangPair::en_sv()].contains(&m.pair()))
        .collect()
}

/// Import both Swedish models from the conversion output unless they're installed already.
fn ensure_models(manifest: &Manifest, store: &ModelStore) {
    let from = std::env::var_os("POLYGLASS_OPUS_MT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/opus-mt/out")
        });
    for m in swedish_models(manifest) {
        if !store.is_installed(m) {
            store
                .import_from_dir(m, &from.join(&m.id))
                .unwrap_or_else(|e| {
                    panic!(
                        "{}: {e}. Run `python scripts/models/opus_mt.py --no-install` first",
                        m.id
                    )
                });
        }
    }
}

fn backend() -> Ct2Translator {
    let manifest = Manifest::bundled().unwrap();
    let store = store();
    ensure_models(&manifest, &store);
    let start = Instant::now();
    let t = Ct2Translator::load_installed(&manifest, &store, Options::default()).unwrap();
    eprintln!(
        "loaded {:?} in {:?} ({} threads)",
        t.pairs(),
        start.elapsed(),
        t.options().threads
    );
    t
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

#[test]
#[ignore = "needs the converted models (scripts/models/opus_mt.py)"]
fn mdl04_tr2_converted_models_import_and_verify() {
    let manifest = Manifest::bundled().unwrap();
    let store = store();
    ensure_models(&manifest, &store);
    let models = swedish_models(&manifest);
    assert_eq!(models.len(), 2);
    for m in models {
        assert!(store.is_installed(m), "{}", m.id);
        assert_eq!(m.license, "CC-BY-4.0");
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the converted models (scripts/models/opus_mt.py)"]
async fn tr1_tr3_both_directions_loaded_and_translate() {
    let t = TokenGuard::new(backend());
    assert_eq!(t.id(), "ctranslate2");
    assert!(t.is_local(), "TR-01: on-device");
    assert!(t.supports(&LangPair::sv_en()) && t.supports(&LangPair::en_sv()));

    // SV-01 / TR-3: both models are already loaded, so swapping direction is just a call.
    for round in 0..3 {
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
        eprintln!("swap {round}: {en:?} / {sv:?} in {both:?}");
        assert_eq!(en, ["Save changes?"]);
        assert!(sv[0].contains("ändringar"), "{sv:?}");
        assert!(
            both.as_millis() < 500,
            "swap + 2 short strings took {both:?}"
        );
    }

    for (name, pair) in [
        ("ui-dialog-sv", LangPair::sv_en()),
        ("ui-dialog-en", LangPair::en_sv()),
    ] {
        let blocks = fixture(name);
        let start = Instant::now();
        let out = t.translate(&blocks, &pair).await.unwrap();
        eprintln!("{name} ({} blocks): {:?}", blocks.len(), start.elapsed());
        for (s, o) in blocks.iter().zip(&out) {
            eprintln!("  {s} -> {o}");
        }
        assert_eq!(out.len(), blocks.len());
    }
}

/// The SV-05 guard's masked retry relies on `{n}` placeholders coming out of the model
/// unchanged. Check that on every maskable block of the numbers fixtures, without the guard.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the converted models (scripts/models/opus_mt.py)"]
async fn sv05_placeholders_survive_opus_mt() {
    let t = backend();
    for (name, pair) in [
        ("numbers-sv", LangPair::sv_en()),
        ("numbers-en", LangPair::en_sv()),
    ] {
        let masked: Vec<_> = fixture(name).iter().filter_map(|b| mask(b)).collect();
        let texts: Vec<String> = masked.iter().map(|m| m.text.clone()).collect();
        let out = t.translate(&texts, &pair).await.unwrap();
        let mut lost = Vec::new();
        for (m, o) in masked.iter().zip(&out) {
            eprintln!("  {} -> {o}", m.text);
            if unmask(o, m, &pair).is_none() {
                lost.push(o.clone());
            }
        }
        eprintln!(
            "{name}: {}/{} kept every placeholder",
            masked.len() - lost.len(),
            masked.len()
        );
        assert!(lost.is_empty(), "{name}: placeholders lost in {lost:?}");
    }
}

/// SV-05 on the real backend: every block of `numbers-sv` (sv→en) and `numbers-en` (en→sv)
/// keeps its digits, values, URLs and e-mail addresses.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the converted models (scripts/models/opus_mt.py)"]
async fn tr4_sv05_numbers_fixtures_keep_preserved_tokens() {
    let t = TokenGuard::new(backend());
    for (name, pair) in [
        ("numbers-sv", LangPair::sv_en()),
        ("numbers-en", LangPair::en_sv()),
    ] {
        let blocks = fixture(name);
        let (retries, fallbacks) = (t.masked_retries(), t.fallbacks());
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
            "{name}: masked retries {}, fallbacks {}, untranslated {untranslated}/{}",
            t.masked_retries() - retries,
            t.fallbacks() - fallbacks,
            blocks.len()
        );
        assert!(
            untranslated <= 2,
            "{name}: {untranslated} blocks untranslated: {out:?}"
        );
    }
}

/// Nearest-rank percentile.
fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Resident memory of this process in MB, if the OS tells us.
fn rss_mb() -> Option<f64> {
    if cfg!(windows) {
        let pid = std::process::id();
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(Get-Process -Id {pid}).WorkingSet64"),
            ])
            .output()
            .ok()?;
        let bytes: f64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
        Some(bytes / 1e6)
    } else if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        let kb: f64 = status
            .lines()
            .find(|l| l.starts_with("VmRSS:"))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()?;
        Some(kb / 1e3)
    } else {
        // macOS has no /proc; `ps` reports the resident size in KiB.
        let pid = std::process::id().to_string();
        let out = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &pid])
            .output()
            .ok()?;
        let kib: f64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
        Some(kib * 1024.0 / 1e6)
    }
}

/// PERF-01/06 inputs and RES-04: latency per direction (3 warm-up runs, then 30 timed runs) and
/// memory with both directions loaded. Prints the numbers; asserts only the SPEC bounds.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "measurement; needs the converted models (scripts/models/opus_mt.py)"]
async fn perf_latency_and_memory_both_directions() {
    let before = rss_mb();
    let raw = backend();
    let loaded = rss_mb();
    let t = TokenGuard::new(raw);
    let sv_para = fixture("paragraphs-sv")[1].clone();
    let en_para = fixture("paragraphs-en")[1].clone();
    let cases = [
        (
            "sv→en short",
            LangPair::sv_en(),
            vec!["Spara ändringar?".to_owned()],
        ),
        ("sv→en paragraph", LangPair::sv_en(), vec![sv_para]),
        (
            "sv→en ui-dialog-sv",
            LangPair::sv_en(),
            fixture("ui-dialog-sv"),
        ),
        (
            "en→sv short",
            LangPair::en_sv(),
            vec!["Save changes?".to_owned()],
        ),
        ("en→sv paragraph", LangPair::en_sv(), vec![en_para]),
        (
            "en→sv ui-dialog-en",
            LangPair::en_sv(),
            fixture("ui-dialog-en"),
        ),
    ];
    for (name, pair, blocks) in &cases {
        for _ in 0..3 {
            t.translate(blocks, pair).await.unwrap();
        }
        let mut times: Vec<Duration> = Vec::new();
        for _ in 0..30 {
            let start = Instant::now();
            t.translate(blocks, pair).await.unwrap();
            times.push(start.elapsed());
        }
        times.sort();
        let (p50, p95) = (percentile(&times, 50.0), percentile(&times, 95.0));
        eprintln!("{name}: p50 {p50:.1?}, p95 {p95:.1?}");
        if name.contains("ui-dialog") {
            // PERF-01/06 budget for the whole cycle is p50 ≤ 600 ms; translation must fit well inside.
            assert!(p50 < Duration::from_millis(600), "{name}: p50 {p50:?}");
        }
    }
    let after = rss_mb();
    eprintln!("RSS MB: before load {before:?}, both loaded {loaded:?}, after runs {after:?}");
    if let Some(mb) = after {
        assert!(mb < 400.0, "RES-04: {mb} MB");
    }
}

/// M3-9: German → English from its catalog entry alone. Skips unless the German model was
/// converted (`python scripts/models/opus_mt.py opus-mt-de-en --no-install`), since the default
/// run (and the `Measure` workflow) converts only the Swedish pair.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the converted models (scripts/models/opus_mt.py)"]
async fn m3_9_german_to_english_translates_and_keeps_numbers() {
    let manifest = Manifest::bundled().unwrap();
    let de_en = LangPair::new(
        pg_core::LangTag::parse("de").unwrap(),
        pg_core::LangTag::en(),
    );
    let entry = manifest.models.iter().find(|m| m.pair() == de_en).unwrap();
    let from = std::env::var_os("POLYGLASS_OPUS_MT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/opus-mt/out")
        })
        .join(&entry.id);
    let store = store();
    if !store.is_installed(entry) {
        if !from.is_dir() {
            eprintln!("skipped: {} isn't converted", entry.id);
            return;
        }
        store.import_from_dir(entry, &from).unwrap();
    }
    let t = TokenGuard::new(
        Ct2Translator::load_installed(&manifest, &store, Options::default()).unwrap(),
    );
    assert!(t.supports(&de_en));
    let out = t
        .translate(
            &[
                "Möchten Sie die Änderungen speichern?".into(),
                "Gesamtbetrag: 1.234,50 €".into(),
            ],
            &de_en,
        )
        .await
        .unwrap();
    eprintln!("{out:?}");
    assert!(out[0].to_lowercase().contains("save"), "{out:?}");
    assert!(
        check_preserved(
            "Gesamtbetrag: 1.234,50 €",
            &de_en.source,
            &out[1],
            &de_en.target
        )
        .is_ok(),
        "{out:?}"
    );
}
