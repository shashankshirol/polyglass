//! Write the fixture images to disk for inspection (M0-FIX-2).
//!
//! ```text
//! cargo run -p pg-testkit --bin render-fixtures -- [OUT_DIR] [FIXTURE...]
//! ```
//!
//! Writes `OUT_DIR/<fixture>/<fixture>@<scale>-<theme>.png` plus a `.json` with the font and the
//! ground-truth line boxes, for every fixture (or the named ones) at every scale and theme, and
//! prints each PNG's SHA-256. `OUT_DIR` defaults to `target/fixture-images`. Tests and `pg-bench`
//! don't read these files: they render the same images in memory.

#![allow(clippy::print_stdout)]

use std::path::PathBuf;
use std::process::ExitCode;

use pg_testkit::TextRenderer;
use pg_testkit::fixture::{self, FixtureImage, RenderedLine, SCALES, Theme};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Serialize)]
struct Sidecar<'a> {
    fixture: &'a str,
    scale: f64,
    theme: Theme,
    width: u32,
    height: u32,
    font: String,
    font_sha256: &'a str,
    png_sha256: &'a str,
    lines: &'a [RenderedLine],
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("render-fixtures: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/fixture-images".to_owned()),
    );
    let only: Vec<String> = args.collect();
    let r = TextRenderer::system().ok_or("no system UI font found")?;
    println!(
        "font {} sha256 {}",
        r.font_path().display(),
        r.font_sha256()
    );
    let truths = fixture::all().map_err(|e| e.to_string())?;
    for truth in truths
        .iter()
        .filter(|t| only.is_empty() || only.contains(&t.fixture))
    {
        let dir = out.join(&truth.fixture);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for scale in SCALES {
            for theme in Theme::ALL {
                let img = FixtureImage::render(&r, truth, scale, theme);
                let png = img.png();
                let sha = hex::encode(Sha256::digest(&png));
                let sidecar = Sidecar {
                    fixture: &img.fixture,
                    scale,
                    theme,
                    width: img.frame.width(),
                    height: img.frame.height(),
                    font: r.font_path().display().to_string(),
                    font_sha256: r.font_sha256(),
                    png_sha256: &sha,
                    lines: &img.lines,
                };
                let stem = dir.join(img.stem());
                let png_path = stem.with_extension("png");
                std::fs::write(&png_path, &png)
                    .map_err(|e| format!("{}: {e}", png_path.display()))?;
                let json = serde_json::to_string_pretty(&sidecar).map_err(|e| e.to_string())?;
                std::fs::write(stem.with_extension("json"), json + "\n")
                    .map_err(|e| format!("{}: {e}", stem.display()))?;
                println!("{sha}  {}", png_path.display());
            }
        }
    }
    Ok(())
}
