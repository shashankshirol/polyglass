//! `pg-bench`: run the pipeline on a fixture and print latency percentiles (SPEC §5).
//!
//! The lens sits at the top-left of the fixture image that `pg-testkit` renders from truth.json
//! at `--scale` in `--theme` (M0-FIX-2), served by `FixtureCapturer`. `--ocr mock` replays the
//! ground truth so the pipeline overhead itself can be measured and regression-gated (QUAL-02);
//! `--ocr windows` runs Windows OCR on the pixels and also reports its accuracy on the whole lines
//! inside the lens (CER, WER, å/ä/ö; M0-FIX-3). Translation is still `MockTranslator`.

#![allow(clippy::print_stdout)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, ValueEnum};
use pg_capture::{FixtureCapturer, ScreenCapturer};
use pg_core::{LangTag, Line, SourceLang};
use pg_ocr::{MockOcr, OcrEngine};
use pg_pipeline::{CycleOutcome, Pipeline, PipelineConfig, Tick, Timings};
use pg_testkit::TextRenderer;
use pg_testkit::fixture::{FixtureImage, Theme, Truth};
use pg_testkit::metrics::{Score, score};
use pg_translate::MockTranslator;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Ocr {
    /// Replay ground-truth text from truth.json.
    Mock,
    /// Windows OCR (`Windows.Media.Ocr`) on the fixture pixels.
    #[cfg(windows)]
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ThemeArg {
    Light,
    Dark,
}

impl From<ThemeArg> for Theme {
    fn from(t: ThemeArg) -> Theme {
        match t {
            ThemeArg::Light => Theme::Light,
            ThemeArg::Dark => Theme::Dark,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Cache {
    /// Clear the translation cache before every run (PERF-01, PERF-06).
    Uncached,
    /// Warm cache (PERF-02).
    Cached,
}

#[derive(Debug, Parser)]
#[command(name = "pg-bench", about = "Polyglass pipeline benchmark")]
struct Args {
    /// Fixture directory containing truth.json.
    #[arg(long, default_value = "tests/fixtures/ui-dialog-sv")]
    fixture: PathBuf,
    /// Target language.
    #[arg(long, default_value = "en")]
    target: String,
    #[arg(long, value_enum, default_value = "mock")]
    ocr: Ocr,
    #[arg(long, value_enum, default_value = "uncached")]
    cache: Cache,
    /// Simulated OCR latency in ms for the mock engine.
    #[arg(long, default_value_t = 0)]
    mock_ocr_ms: u64,
    #[arg(long, default_value_t = 5)]
    warmup: usize,
    #[arg(long, default_value_t = 100)]
    runs: usize,
    /// Lens size in logical px (SPEC §5: 600×300 at 100%).
    #[arg(long, default_value_t = 600)]
    width: u32,
    #[arg(long, default_value_t = 300)]
    height: u32,
    /// Monitor scale the fixture is rendered at (1.0 = 100%).
    #[arg(long, default_value_t = 1.0)]
    scale: f64,
    /// Colour theme of the fixture image.
    #[arg(long, value_enum, default_value = "light")]
    theme: ThemeArg,
    /// Print JSON instead of a table.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Serialize)]
struct StageStats {
    stage: &'static str,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
}

#[derive(Debug, Serialize)]
struct Report {
    fixture: String,
    /// Which rendering, e.g. `ui-dialog-sv@100-light`.
    image: String,
    pair: String,
    ocr: String,
    cache: String,
    runs: usize,
    stages: Vec<StageStats>,
    /// OCR accuracy on the whole lines inside the lens; real OCR engines only.
    #[serde(skip_serializing_if = "Option::is_none")]
    accuracy: Option<Score>,
}

/// Nearest-rank percentile of sorted values.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn stats(stage: &'static str, mut v: Vec<f64>) -> StageStats {
    v.sort_by(f64::total_cmp);
    StageStats {
        stage,
        p50_ms: percentile(&v, 50.0),
        p95_ms: percentile(&v, 95.0),
        p99_ms: percentile(&v, 99.0),
        max_ms: v.last().copied().unwrap_or(0.0),
    }
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pg-bench: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    let truth = Truth::load(&args.fixture).map_err(|e| e.to_string())?;
    let target = LangTag::parse(&args.target).map_err(|e| e.to_string())?;

    let region = pg_core::LogicalRect::new(0.0, 0.0, f64::from(args.width), f64::from(args.height))
        .to_physical(args.scale);
    let r = TextRenderer::system().ok_or("no system UI font found to render the fixture")?;
    let img = FixtureImage::render(&r, &truth, args.scale, args.theme.into());
    let mut capturer =
        FixtureCapturer::from_png(&img.png(), args.scale).map_err(|e| e.to_string())?;
    // The whole lines inside the lens: what mock OCR replays and what accuracy is scored on.
    let in_lens = img
        .bands(region.height)
        .into_iter()
        .next()
        .ok_or("the fixture has no text")?;
    let ocr: Arc<dyn OcrEngine> = match args.ocr {
        Ocr::Mock => {
            let lines = img.lines[in_lens.lines.clone()]
                .iter()
                .map(|l| Line::new(l.text.clone(), l.bbox, 0.95))
                .collect();
            Arc::new(MockOcr::fixed(lines).with_delay(Duration::from_millis(args.mock_ocr_ms)))
        }
        #[cfg(windows)]
        Ocr::Windows => Arc::new(pg_ocr::WindowsOcr::new().map_err(|e| e.to_string())?),
    };
    let accuracy = if args.ocr == Ocr::Mock {
        None
    } else {
        // Leave out a line cut by the lens's bottom edge; it isn't in the reference.
        let lens = capturer.capture(region).map_err(|e| e.to_string())?;
        let strip = lens.crop(0, 0, lens.width(), in_lens.height);
        let got: Vec<String> = ocr
            .recognize(&strip, Some(&truth.ocr_lang()))
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|l| l.text)
            .collect();
        Some(score(&got.join("\n"), &img.text_of(in_lens.lines)))
    };
    let source = match &truth.lang {
        Some(lang) => SourceLang::Fixed(lang.clone()),
        None => SourceLang::AutoSvEn,
    };
    let translator = MockTranslator::new();
    let config = PipelineConfig::new(source, target.clone());
    let pipeline = Pipeline::new(Box::new(capturer), ocr, Arc::new(translator), config);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_time()
        .build()
        .map_err(|e| e.to_string())?;
    let mut samples: Vec<Timings> = Vec::with_capacity(args.runs);
    rt.block_on(async {
        for i in 0..args.warmup + args.runs {
            if args.cache == Cache::Uncached {
                pipeline.clear_cache();
            }
            let tick = Tick {
                generation: i as u64,
                region,
                force: true,
            };
            match pipeline.run_cycle(tick).await {
                CycleOutcome::Update(u) if i >= args.warmup => samples.push(u.timings),
                CycleOutcome::Update(_) => {}
                CycleOutcome::Unchanged => return Err("unexpected unchanged frame".to_string()),
            }
        }
        Ok(())
    })?;

    let ms = |f: fn(&Timings) -> Duration| -> Vec<f64> {
        samples
            .iter()
            .map(|t| f(t).as_secs_f64() * 1000.0)
            .collect()
    };
    let report = Report {
        fixture: args.fixture.display().to_string(),
        image: img.stem(),
        pair: format!(
            "{}→{}",
            truth.lang.as_ref().map_or("auto", LangTag::as_str),
            target
        ),
        ocr: format!("{:?}", args.ocr).to_lowercase(),
        cache: format!("{:?}", args.cache).to_lowercase(),
        runs: samples.len(),
        stages: vec![
            stats("capture", ms(|t| t.capture)),
            stats("change", ms(|t| t.change)),
            stats("ocr", ms(|t| t.ocr)),
            stats("group", ms(|t| t.group)),
            stats("translate", ms(|t| t.translate)),
            stats("total", ms(|t| t.total)),
        ],
        accuracy,
    };

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{} ({})  {}  ocr={}  cache={}  runs={}",
            report.fixture, report.image, report.pair, report.ocr, report.cache, report.runs
        );
        println!(
            "{:<10} {:>9} {:>9} {:>9} {:>9}",
            "stage", "p50 ms", "p95 ms", "p99 ms", "max ms"
        );
        for s in &report.stages {
            println!(
                "{:<10} {:>9.3} {:>9.3} {:>9.3} {:>9.3}",
                s.stage, s.p50_ms, s.p95_ms, s.p99_ms, s.max_ms
            );
        }
        if let Some(a) = &report.accuracy {
            println!("accuracy   {a}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_percentiles() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&v, 50.0), 50.0);
        assert_eq!(percentile(&v, 95.0), 95.0);
        assert_eq!(percentile(&v, 100.0), 100.0);
        assert_eq!(percentile(&[7.0], 50.0), 7.0);
        assert_eq!(percentile(&[], 50.0), 0.0);
    }
}
