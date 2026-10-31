//! `pg-bench`: run the pipeline on a fixture and print latency percentiles (SPEC §5).
//!
//! Real OCR engines and the on-device translator plug in here as they land (CHECKLIST
//! M1-OCR-*, M1-TR-1); until then `--ocr mock` replays the fixture's ground truth so the
//! pipeline overhead itself can be measured and regression-gated (QUAL-02).

#![allow(clippy::print_stdout)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, ValueEnum};
use pg_capture::{FixtureCapturer, ScreenCapturer, StaticCapturer};
use pg_core::{Frame, LangTag, Line, PhysRect, Rect, SourceLang};
use pg_ocr::MockOcr;
use pg_pipeline::{CycleOutcome, Pipeline, PipelineConfig, Tick, Timings};
use pg_translate::MockTranslator;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Ocr {
    /// Replay ground-truth text from truth.json.
    Mock,
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
    /// Fixture directory containing truth.json (and the image once generated).
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
    #[arg(long, default_value_t = 1.0)]
    scale: f64,
    /// Print JSON instead of a table.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Deserialize)]
struct Truth {
    lang: LangTag,
    image: Option<String>,
    blocks: Vec<TruthBlock>,
}

#[derive(Debug, Deserialize)]
struct TruthBlock {
    text: String,
    #[serde(default)]
    bbox: Option<Rect>,
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
    pair: String,
    ocr: String,
    cache: String,
    runs: usize,
    stages: Vec<StageStats>,
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

/// Lay truth blocks out top to bottom when they have no boxes yet (no images, M0-FIX-2).
fn mock_lines(truth: &Truth, scale: f32) -> Vec<Line> {
    let mut y = 10.0;
    truth
        .blocks
        .iter()
        .map(|b| {
            let bbox = b.bbox.map(|r| r.scaled(scale)).unwrap_or_else(|| {
                let r = Rect::new(10.0 * scale, y * scale, 400.0 * scale, 14.0 * scale);
                y += 40.0;
                r
            });
            Line::new(b.text.clone(), bbox, 0.95)
        })
        .collect()
}

/// A synthetic desktop with dark bars where the lines are, so the frame isn't blank.
fn synthetic_desktop(lines: &[Line], w: u32, h: u32) -> Frame {
    let mut f = Frame::solid(w, h, [250, 250, 250]);
    let stride = f.stride();
    let data = f.data_mut();
    for l in lines {
        let (x0, y0) = (l.bbox.x as u32, l.bbox.y as u32);
        for y in y0..(y0 + l.bbox.h as u32).min(h) {
            for x in x0..(x0 + l.bbox.w as u32).min(w) {
                let i = y as usize * stride + x as usize * 4;
                data[i..i + 3].copy_from_slice(&[30, 30, 30]);
            }
        }
    }
    f
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
    let truth_path = args.fixture.join("truth.json");
    let truth: Truth = serde_json::from_str(
        &std::fs::read_to_string(&truth_path)
            .map_err(|e| format!("{}: {e}", truth_path.display()))?,
    )
    .map_err(|e| format!("{}: {e}", truth_path.display()))?;
    let target = LangTag::parse(&args.target).map_err(|e| e.to_string())?;

    let region = pg_core::LogicalRect::new(0.0, 0.0, f64::from(args.width), f64::from(args.height))
        .to_physical(args.scale);
    let lines = mock_lines(&truth, args.scale as f32);
    let capturer: Box<dyn ScreenCapturer> = match &truth.image {
        Some(img) => Box::new(
            FixtureCapturer::open(&args.fixture.join(img), args.scale)
                .map_err(|e| e.to_string())?,
        ),
        None => Box::new(StaticCapturer::new(
            synthetic_desktop(&lines, region.width, region.height)
                .with_origin(PhysRect::new(0, 0, region.width, region.height), args.scale),
        )),
    };
    let ocr = match args.ocr {
        Ocr::Mock => MockOcr::fixed(lines).with_delay(Duration::from_millis(args.mock_ocr_ms)),
    };
    let translator = MockTranslator::new();
    let config = PipelineConfig::new(SourceLang::Fixed(truth.lang.clone()), target.clone());
    let pipeline = Pipeline::new(capturer, Arc::new(ocr), Arc::new(translator), config);

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
        pair: format!("{}→{}", truth.lang, target),
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
    };

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}  {}  ocr={}  cache={}  runs={}",
            report.fixture, report.pair, report.ocr, report.cache, report.runs
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
