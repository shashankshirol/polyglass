//! Developer flags (M1-APP-10). Neither is for everyday use, and both are off unless passed on
//! the command line (`npm run tauri dev -- -- --trace-timings`, or after `polyglass.exe`):
//!
//! - `--trace-timings` logs every cycle's stage timings and, once the lens has drawn a result,
//!   how long that took from the moment the lens came to rest (PERF-01/02/06 in the real app),
//!   plus a summary at exit. Numbers only, never screen text (PRIV-07).
//!   `--trace-timings=uncached` also empties the translation cache whenever the lens moves, so
//!   every result after coming to rest is translated afresh (PERF-01/06 rather than PERF-02).
//! - `--debug-dump-frames[=DIR]` writes every captured frame to DIR as a PNG: the one exception
//!   to "frames never touch the disk" (PRIV-02). The lens shows a red banner the whole time.
//!   Each run gets its own folder, `DIR/<UTC start time>/` (default DIR:
//!   `<app local data>/debug-frames`), holding its newest [`KEEP_FRAMES`] frames.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::time::{Duration, Instant};

use pg_capture::ScreenCapturer;
use pg_core::{Frame, PhysRect, Result};
use pg_pipeline::{LensUpdate, SchedulerConfig, Timings};

pub const TRACE_TIMINGS: &str = "--trace-timings";
pub const DUMP_FRAMES: &str = "--debug-dump-frames";
/// Frames kept per run; older ones are deleted as new ones arrive.
pub const KEEP_FRAMES: u64 = 200;
/// A translation step shorter than this counts as served from the cache in the trace summary.
const CACHED_BELOW: Duration = Duration::from_millis(1);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DevFlags {
    pub trace_timings: bool,
    /// `--trace-timings=uncached`: clear the translation cache when the lens moves.
    pub trace_uncached: bool,
    /// `Some(None)`: the default folder; `Some(Some(dir))`: under `dir`.
    pub dump_frames: Option<Option<PathBuf>>,
}

impl DevFlags {
    /// Read the flags from the command line; anything else is ignored (Tauri and `--demo`
    /// have their own).
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut flags = DevFlags::default();
        for a in args {
            if a == TRACE_TIMINGS {
                flags.trace_timings = true;
            } else if a.strip_prefix(TRACE_TIMINGS) == Some("=uncached") {
                flags.trace_timings = true;
                flags.trace_uncached = true;
            } else if a == DUMP_FRAMES {
                flags.dump_frames = Some(None);
            } else if let Some(dir) = a
                .strip_prefix(DUMP_FRAMES)
                .and_then(|r| r.strip_prefix('='))
                && !dir.is_empty()
            {
                flags.dump_frames = Some(Some(PathBuf::from(dir)));
            }
        }
        flags
    }
}

/// `YYYY-MM-DDTHH-MM-SSZ` for `t` (UTC, file-name safe).
pub fn utc_stamp(t: std::time::SystemTime) -> String {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Days since 1970-01-01 to a civil date (H. Hinnant's `civil_from_days`).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}-{:02}-{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// Wraps the capturer and hands a copy of every frame to a writer thread, which saves it as
/// `frame-NNNNNN.png` (`--debug-dump-frames`). A frame is dropped rather than slowing capture
/// when the writer falls behind.
pub struct DumpFrames<C> {
    inner: C,
    tx: SyncSender<Frame>,
}

impl<C> std::fmt::Debug for DumpFrames<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DumpFrames").finish_non_exhaustive()
    }
}

impl<C: ScreenCapturer> DumpFrames<C> {
    /// Frames go to `dir`, which is created here.
    pub fn new(inner: C, dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let (tx, rx) = sync_channel::<Frame>(8);
        std::thread::Builder::new()
            .name("debug-dump-frames".into())
            .spawn(move || {
                let mut seq: u64 = 0;
                for frame in rx {
                    seq += 1;
                    if let Err(error) = write_png(&frame, &frame_path(&dir, seq)) {
                        tracing::warn!(%error, "can't write a debug frame");
                    }
                    if seq > KEEP_FRAMES {
                        let _ = std::fs::remove_file(frame_path(&dir, seq - KEEP_FRAMES));
                    }
                }
            })?;
        Ok(Self { inner, tx })
    }
}

impl<C: ScreenCapturer> ScreenCapturer for DumpFrames<C> {
    fn capture(&mut self, region: PhysRect) -> Result<Frame> {
        let frame = self.inner.capture(region)?;
        if let Err(TrySendError::Full(_)) = self.tx.try_send(frame.clone()) {
            tracing::debug!("debug frame writer is behind; frame skipped");
        }
        Ok(frame)
    }

    fn excludes_self(&self) -> bool {
        self.inner.excludes_self()
    }

    fn release(&mut self) {
        self.inner.release();
    }
}

/// `capturer`, saving its frames into `dump` if `--debug-dump-frames` is on.
pub fn capturer(
    capturer: impl ScreenCapturer + 'static,
    dump: Option<PathBuf>,
) -> Result<Box<dyn ScreenCapturer>> {
    Ok(match dump {
        Some(dir) => {
            tracing::warn!(dir = %dir.display(), "--debug-dump-frames: saving every captured frame");
            Box::new(
                DumpFrames::new(capturer, dir)
                    .map_err(|e| pg_core::Error::Capture(format!("--debug-dump-frames: {e}")))?,
            )
        }
        None => Box::new(capturer),
    })
}

fn frame_path(dir: &Path, seq: u64) -> PathBuf {
    dir.join(format!("frame-{seq:06}.png"))
}

fn write_png(frame: &Frame, path: &Path) -> image::ImageResult<()> {
    let (w, h) = (frame.width(), frame.height());
    let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
    for y in 0..h {
        for x in 0..w {
            rgb.extend_from_slice(&frame.rgb(x, y));
        }
    }
    image::save_buffer(path, &rgb, w, h, image::ColorType::Rgb8)
}

/// `--trace-timings`: pairs each update sent to the lens with the lens's report that it drew
/// it, and logs the timings under the `pg::trace` target.
#[derive(Debug)]
pub struct Trace {
    rest_delay: Duration,
    uncached: bool,
    inner: Mutex<TraceState>,
}

#[derive(Debug, Default)]
struct TraceState {
    /// When the lens last moved, was resized or was shown.
    moved: Option<Instant>,
    /// Updates sent to the lens and not drawn yet: generation, when, stage timings.
    pending: VecDeque<(u64, Instant, Timings)>,
    /// The last generation drawn: the next one is a result after coming to rest.
    drawn_generation: Option<u64>,
    /// At rest → drawn, ms, split by whether translation came from the cache.
    uncached: Vec<f64>,
    cached: Vec<f64>,
}

/// One drawn update.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drawn {
    pub generation: u64,
    /// Cycle start → result sent to the lens (the pipeline's `total`).
    pub cycle_ms: f64,
    /// Sent → drawn: the IPC hop, rendering and font fitting, up to the next painted frame.
    pub deliver_ms: f64,
    /// At rest (the lens stopped moving + the rest delay) → drawn; only for the first result
    /// after coming to rest, not for re-runs when content under the lens changed.
    pub at_rest_ms: Option<f64>,
    pub cached: bool,
}

const MS: f64 = 1000.0;

impl Trace {
    /// `uncached`: `--trace-timings=uncached`.
    pub fn new(cfg: &SchedulerConfig, uncached: bool) -> Self {
        Self {
            rest_delay: cfg.rest_delay,
            uncached,
            inner: Mutex::new(TraceState::default()),
        }
    }

    /// Whether the translation cache is emptied whenever the lens moves.
    pub fn uncached(&self) -> bool {
        self.uncached
    }

    /// The lens moved, was resized or was shown: a new "at rest" moment follows.
    pub fn lens_moved(&self, now: Instant) {
        self.inner.lock().expect("trace lock").moved = Some(now);
    }

    /// An update is about to be sent to the lens.
    pub fn sent(&self, u: &LensUpdate, now: Instant) {
        let t = &u.timings;
        tracing::info!(
            target: "pg::trace",
            generation = u.generation,
            blocks = u.blocks.len(),
            capture_ms = t.capture.as_secs_f64() * MS,
            change_ms = t.change.as_secs_f64() * MS,
            ocr_ms = t.ocr.as_secs_f64() * MS,
            group_ms = t.group.as_secs_f64() * MS,
            translate_ms = t.translate.as_secs_f64() * MS,
            total_ms = t.total.as_secs_f64() * MS,
            "cycle"
        );
        let mut s = self.inner.lock().expect("trace lock");
        s.pending.push_back((u.generation, now, *t));
        // The lens reports every update it draws; this only bounds a lens that stopped.
        while s.pending.len() > 32 {
            s.pending.pop_front();
        }
    }

    /// The lens drew the update of `generation` (and skipped any older one still pending).
    pub fn drawn(&self, generation: u64, now: Instant) -> Option<Drawn> {
        let mut s = self.inner.lock().expect("trace lock");
        let (sent, timings) = loop {
            let (g, sent, timings) = s.pending.pop_front()?;
            if g == generation {
                break (sent, timings);
            }
        };
        let deliver = now.saturating_duration_since(sent);
        let cached = timings.translate < CACHED_BELOW;
        let first = s.drawn_generation != Some(generation);
        s.drawn_generation = Some(generation);
        let at_rest = s.moved.filter(|_| first).map(|m| {
            now.saturating_duration_since(m + self.rest_delay)
                .as_secs_f64()
                * MS
        });
        if let Some(ms) = at_rest {
            if cached {
                s.cached.push(ms);
            } else {
                s.uncached.push(ms);
            }
        }
        let d = Drawn {
            generation,
            cycle_ms: timings.total.as_secs_f64() * MS,
            deliver_ms: deliver.as_secs_f64() * MS,
            at_rest_ms: at_rest,
            cached,
        };
        tracing::info!(
            target: "pg::trace",
            generation,
            cycle_ms = d.cycle_ms,
            deliver_ms = d.deliver_ms,
            at_rest_ms = d.at_rest_ms,
            cached,
            "drawn"
        );
        Some(d)
    }

    /// At rest → drawn percentiles so far (nearest rank): (count, p50, p95) uncached, cached.
    pub fn summary(&self) -> [(usize, f64, f64); 2] {
        let s = self.inner.lock().expect("trace lock");
        [percentiles(&s.uncached), percentiles(&s.cached)]
    }

    pub fn log_summary(&self) {
        let [(n, p50, p95), (cn, cp50, cp95)] = self.summary();
        tracing::info!(
            target: "pg::trace",
            uncached = n,
            uncached_p50_ms = p50,
            uncached_p95_ms = p95,
            cached = cn,
            cached_p50_ms = cp50,
            cached_p95_ms = cp95,
            "at rest → drawn"
        );
    }
}

fn percentiles(v: &[f64]) -> (usize, f64, f64) {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    let at = |p: f64| {
        if v.is_empty() {
            return 0.0;
        }
        let rank = ((p / 100.0) * v.len() as f64).ceil() as usize;
        v[rank.clamp(1, v.len()) - 1]
    };
    (v.len(), at(50.0), at(95.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pg_core::PixelFormat;
    use pg_pipeline::{BackendInfo, LensStatus};

    fn args(a: &[&str]) -> DevFlags {
        DevFlags::parse(a.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn flags_are_off_unless_passed() {
        assert_eq!(args(&["polyglass", "--demo"]), DevFlags::default());
        let trace = args(&["polyglass", TRACE_TIMINGS]);
        assert!(trace.trace_timings && !trace.trace_uncached);
        let uncached = args(&["--trace-timings=uncached"]);
        assert!(uncached.trace_timings && uncached.trace_uncached);
        assert!(!args(&["--trace-timings=other"]).trace_timings);
        assert_eq!(
            args(&["polyglass", DUMP_FRAMES]).dump_frames,
            Some(None),
            "default folder"
        );
        assert_eq!(
            args(&["polyglass", "--debug-dump-frames=C:\\frames"]).dump_frames,
            Some(Some(PathBuf::from("C:\\frames")))
        );
        // Neither a prefix nor an empty folder turns it on.
        assert_eq!(
            args(&["--debug-dump-framesX", "--debug-dump-frames="]).dump_frames,
            None
        );
    }

    #[test]
    fn utc_stamps() {
        let at = |s: u64| utc_stamp(std::time::UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(at(0), "1970-01-01T00-00-00Z");
        assert_eq!(at(951_782_400), "2000-02-29T00-00-00Z");
        assert_eq!(at(1_793_534_461), "2026-11-01T12-01-01Z");
    }

    struct Fixed(Frame);
    impl ScreenCapturer for Fixed {
        fn capture(&mut self, _: PhysRect) -> Result<Frame> {
            Ok(self.0.clone())
        }
        fn excludes_self(&self) -> bool {
            true
        }
    }

    #[test]
    fn priv02_dumped_frames_are_the_captured_pixels() {
        let dir = tempfile::tempdir().unwrap();
        // 2×1 BGRA: red, then blue.
        let frame = Frame::new(
            2,
            1,
            8,
            PixelFormat::Bgra8,
            vec![0, 0, 255, 255, 255, 0, 0, 255],
        )
        .unwrap();
        let mut c = DumpFrames::new(Fixed(frame), dir.path().to_owned()).unwrap();
        assert!(c.excludes_self());
        for _ in 0..3 {
            c.capture(PhysRect::new(0, 0, 2, 1)).unwrap();
        }
        drop(c); // closes the channel; the writer finishes its queue
        let path = frame_path(dir.path(), 3);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(50));
        let img = image::open(&path).unwrap().to_rgb8();
        assert_eq!(img.dimensions(), (2, 1));
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0]);
        assert_eq!(img.get_pixel(1, 0).0, [0, 0, 255]);
    }

    fn update(generation: u64, translate_ms: u64, total_ms: u64) -> LensUpdate {
        LensUpdate {
            generation,
            status: LensStatus::Ok,
            blocks: Vec::new(),
            backend: BackendInfo {
                id: "mock".into(),
                name: "mock".into(),
                local: true,
            },
            notices: Vec::new(),
            timings: Timings {
                translate: Duration::from_millis(translate_ms),
                total: Duration::from_millis(total_ms),
                ..Timings::default()
            },
        }
    }

    #[test]
    fn trace_measures_from_rest_to_drawn() {
        let cfg = SchedulerConfig::default();
        let t = Trace::new(&cfg, false);
        let t0 = Instant::now();
        t.lens_moved(t0);
        let rest = t0 + cfg.rest_delay;
        // At rest → cycle (200 ms) → sent → drawn 20 ms later.
        t.sent(&update(1, 180, 200), rest + Duration::from_millis(200));
        let d = t.drawn(1, rest + Duration::from_millis(220)).unwrap();
        assert!((d.at_rest_ms.unwrap() - 220.0).abs() < 1e-6, "{d:?}");
        assert!((d.deliver_ms - 20.0).abs() < 1e-6 && !d.cached, "{d:?}");
        // A re-run in the same generation (content changed) has no at-rest time.
        t.sent(&update(1, 0, 10), rest + Duration::from_secs(1));
        assert_eq!(
            t.drawn(1, rest + Duration::from_secs(1))
                .unwrap()
                .at_rest_ms,
            None
        );
        // An older pending update is skipped; a cached result counts separately.
        t.lens_moved(t0 + Duration::from_secs(2));
        let rest = t0 + Duration::from_secs(2) + cfg.rest_delay;
        t.sent(&update(2, 0, 15), rest + Duration::from_millis(15));
        t.sent(&update(3, 0, 12), rest + Duration::from_millis(12));
        let d = t.drawn(3, rest + Duration::from_millis(30)).unwrap();
        assert!(
            d.cached && (d.at_rest_ms.unwrap() - 30.0).abs() < 1e-6,
            "{d:?}"
        );
        assert_eq!(t.drawn(2, rest), None, "already skipped");
        let [(n, p50, _), (cn, cp50, _)] = t.summary();
        assert_eq!((n, cn), (1, 1));
        assert!((p50 - 220.0).abs() < 1e-6 && (cp50 - 30.0).abs() < 1e-6);
    }
}
