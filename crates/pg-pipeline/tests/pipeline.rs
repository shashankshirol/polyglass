//! Pipeline tests with mock capturer / OCR / translator (PLAN §9 "Pipeline tests").

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use pg_capture::{ScreenCapturer, StaticCapturer};
use pg_core::{Frame, LangPair, LangTag, Line, PhysRect, Rect, SourceLang};
use pg_ocr::MockOcr;
use pg_pipeline::{
    CycleOutcome, LensStatus, LensUpdate, LowConfidence, Notice, Pipeline, PipelineConfig,
    SchedulerConfig, Tick, driver,
};
use pg_translate::{MockTranslator, TranslateError, Translator};
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};

const LEFT: PhysRect = PhysRect::new(0, 0, 600, 300);
const RIGHT: PhysRect = PhysRect::new(1000, 0, 600, 300);
const SV_TEXT: &str = "Spara dina ändringar innan du stänger fönstret";
const EN_TEXT: &str = "Your changes have been saved to the cloud";

/// Capturer shared with the test so it can change the desktop and count captures.
#[derive(Clone)]
struct Shared(Arc<Mutex<StaticCapturer>>);

impl ScreenCapturer for Shared {
    fn capture(&mut self, region: PhysRect) -> pg_core::Result<Frame> {
        self.0.lock().unwrap().capture(region)
    }
    fn excludes_self(&self) -> bool {
        true
    }
    fn release(&mut self) {
        self.0.lock().unwrap().release();
    }
}

impl Shared {
    fn captures(&self) -> usize {
        self.0.lock().unwrap().captures()
    }
    fn releases(&self) -> usize {
        self.0.lock().unwrap().releases()
    }
    fn set_desktop(&self, f: Frame) {
        self.0.lock().unwrap().set_desktop(f);
    }
}

/// White 1600×300 desktop with dark "text" bars in both halves. `variant` changes the bars.
fn desktop(variant: u8) -> Frame {
    let mut f = Frame::solid(1600, 300, [255, 255, 255]);
    let stride = f.stride();
    let data = f.data_mut();
    let shade = 20 + variant * 60;
    for (x0, y0) in [(20usize, 20usize), (1020, 20)] {
        for y in y0..y0 + 14 {
            for x in x0..x0 + 300 + usize::from(variant) * 40 {
                let i = y * stride + x * 4;
                data[i..i + 3].copy_from_slice(&[shade, shade, shade]);
            }
        }
    }
    f
}

/// OCR mock: "reads" Swedish text on the left half and English on the right, based on where
/// the frame was captured; variant 1 adds a second paragraph.
fn ocr() -> MockOcr {
    MockOcr::with_fn(|frame| {
        let text = if frame.origin.x < 1000 {
            SV_TEXT
        } else {
            EN_TEXT
        };
        let mut lines = vec![Line::new(text, Rect::new(20.0, 20.0, 300.0, 14.0), 0.95)];
        if frame.rgb(330, 25) != [255, 255, 255] {
            lines.push(Line::new(
                "Ny rad",
                Rect::new(20.0, 100.0, 80.0, 14.0),
                0.95,
            ));
        }
        lines
    })
}

struct Rig {
    capturer: Shared,
    ocr: Arc<MockOcr>,
    translator: Arc<MockTranslator>,
    pipeline: Arc<Pipeline>,
}

fn rig_with(ocr_engine: MockOcr, translator: MockTranslator, config: PipelineConfig) -> Rig {
    let capturer = Shared(Arc::new(Mutex::new(StaticCapturer::new(desktop(0)))));
    let ocr = Arc::new(ocr_engine);
    let translator = Arc::new(translator);
    let pipeline = Arc::new(Pipeline::new(
        Box::new(capturer.clone()),
        ocr.clone(),
        translator.clone(),
        config,
    ));
    Rig {
        capturer,
        ocr,
        translator,
        pipeline,
    }
}

fn rig() -> Rig {
    rig_with(
        ocr(),
        MockTranslator::new(),
        PipelineConfig::new(SourceLang::Fixed(LangTag::sv()), LangTag::en()),
    )
}

fn tick(region: PhysRect) -> Tick {
    Tick {
        generation: 1,
        region,
        force: true,
    }
}

async fn cycle(r: &Rig, region: PhysRect) -> LensUpdate {
    match r.pipeline.run_cycle(tick(region)).await {
        CycleOutcome::Update(u) => *u,
        CycleOutcome::Unchanged => panic!("expected an update"),
    }
}

async fn next_update(rx: &mut mpsc::UnboundedReceiver<LensUpdate>) -> LensUpdate {
    timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("update within 5 s")
        .expect("driver alive")
}

/// Wraps a translator with an async delay, so cycles can be caught in flight.
struct Slow<T>(Arc<T>, Duration);

#[async_trait]
impl<T: Translator> Translator for Slow<T> {
    fn id(&self) -> &str {
        self.0.id()
    }
    fn is_local(&self) -> bool {
        self.0.is_local()
    }
    fn supports(&self, pair: &LangPair) -> bool {
        self.0.supports(pair)
    }
    async fn translate(&self, b: &[String], p: &LangPair) -> pg_translate::Result<Vec<String>> {
        sleep(self.1).await;
        self.0.translate(b, p).await
    }
}

// ---------------------------------------------------------------- single cycles

#[tokio::test]
async fn cycle_translates_and_positions_blocks() {
    let r = rig();
    let u = cycle(&r, LEFT).await;
    assert_eq!(u.status, LensStatus::Ok);
    assert_eq!(u.blocks.len(), 1);
    let b = &u.blocks[0];
    assert_eq!(b.source, SV_TEXT);
    assert_eq!(
        b.translation.as_deref(),
        Some(format!("[en] {SV_TEXT}").as_str())
    );
    assert_eq!((b.rect.x, b.rect.y, b.rect.w), (20.0, 20.0, 300.0));
    assert_eq!(b.bg, "#ffffff");
    assert_eq!(b.fg, "#000000");
    assert!(u.backend.local);
}

#[tokio::test]
async fn cap04_rects_are_logical_at_150_percent() {
    let r = rig();
    r.capturer
        .set_desktop(desktop(0).with_origin(PhysRect::new(0, 0, 1600, 300), 1.5));
    let u = cycle(&r, LEFT).await;
    let b = &u.blocks[0].rect;
    assert!((b.x - 20.0 / 1.5).abs() < 1e-4 && (b.w - 200.0).abs() < 1e-4);
}

#[tokio::test]
async fn cap06_blank_region_reports_no_text() {
    let r = rig();
    r.capturer.set_desktop(Frame::solid(1600, 300, [0, 0, 0]));
    let u = cycle(&r, LEFT).await;
    assert_eq!(u.status, LensStatus::NoTextFound);
    assert_eq!(r.ocr.calls(), 0, "OCR skipped on blank regions");
}

#[tokio::test]
async fn tr07_auto_mode_passes_through_target_language() {
    let r = rig_with(
        ocr(),
        MockTranslator::new(),
        PipelineConfig::new(SourceLang::AutoSvEn, LangTag::en()),
    );
    let u = cycle(&r, RIGHT).await;
    let b = &u.blocks[0];
    assert!(b.passthrough);
    assert_eq!(b.source_lang, LangTag::en());
    assert_eq!(b.translation.as_deref(), Some(EN_TEXT));
    assert_eq!(r.translator.calls(), 0, "nothing sent to the backend");

    let u = cycle(&r, LEFT).await;
    assert!(!u.blocks[0].passthrough);
    assert_eq!(u.blocks[0].source_lang, LangTag::sv());
    assert_eq!(r.translator.calls(), 1);
}

#[tokio::test]
async fn tr04_cache_hit_skips_backend() {
    let r = rig();
    cycle(&r, LEFT).await;
    cycle(&r, LEFT).await;
    assert_eq!(r.translator.calls(), 1);
    assert_eq!(r.pipeline.cache_stats(), (1, 1));
}

#[tokio::test]
async fn tr05_one_request_per_frame_with_duplicates_once() {
    let lines = vec![
        Line::new("Spara", Rect::new(0.0, 0.0, 50.0, 14.0), 0.9),
        Line::new("Avbryt", Rect::new(0.0, 100.0, 50.0, 14.0), 0.9),
        Line::new("Spara", Rect::new(0.0, 200.0, 50.0, 14.0), 0.9),
    ];
    let r = rig_with(
        MockOcr::fixed(lines),
        MockTranslator::new(),
        PipelineConfig::new(SourceLang::Fixed(LangTag::sv()), LangTag::en()),
    );
    let u = cycle(&r, LEFT).await;
    assert_eq!(u.blocks.len(), 3);
    assert_eq!(r.translator.calls(), 1);
    assert_eq!(r.translator.texts_sent(), 2);
    assert_eq!(u.blocks[2].translation.as_deref(), Some("[en] Spara"));
}

#[tokio::test]
async fn tr06_errors_have_distinct_status() {
    let r = rig();
    for (err, id) in [
        (
            TranslateError::Network("refused".into()),
            "tr-error-network",
        ),
        (TranslateError::Timeout, "tr-error-timeout"),
        (TranslateError::Auth, "tr-error-auth"),
        (TranslateError::Quota, "tr-error-quota"),
    ] {
        r.translator.set_failure(Some(err));
        let u = cycle(&r, LEFT).await;
        assert_eq!(
            u.status,
            LensStatus::Error {
                message_id: id,
                lang: None
            }
        );
        assert_eq!(u.blocks[0].translation, None);
        assert_eq!(u.blocks[0].display_text(), SV_TEXT);
    }
}

#[tokio::test]
async fn sv02_auto_mode_falls_back_to_english_ocr() {
    let r = rig_with(
        ocr().with_languages(vec![LangTag::parse("en-US").unwrap()]),
        MockTranslator::new(),
        PipelineConfig::new(SourceLang::AutoSvEn, LangTag::en()),
    );
    let u = cycle(&r, RIGHT).await;
    assert_eq!(u.status, LensStatus::Ok);
    assert_eq!(
        u.notices,
        [Notice::OcrLanguageMissing {
            lang: LangTag::sv()
        }]
    );
}

#[tokio::test]
async fn ocr03_fixed_swedish_missing_is_actionable_error() {
    let r = rig_with(
        ocr().with_languages(vec![LangTag::parse("en-US").unwrap()]),
        MockTranslator::new(),
        PipelineConfig::new(SourceLang::Fixed(LangTag::sv()), LangTag::en()),
    );
    let u = cycle(&r, LEFT).await;
    assert_eq!(
        u.status,
        LensStatus::Error {
            message_id: "ocr-error-language-missing",
            lang: Some(LangTag::sv())
        }
    );
}

#[tokio::test]
async fn ocr05_low_confidence_hide_or_dim() {
    let lines = vec![
        Line::new("Tydlig text", Rect::new(0.0, 0.0, 80.0, 14.0), 0.9),
        Line::new("Suddig", Rect::new(0.0, 100.0, 50.0, 14.0), 0.3),
    ];
    let mut cfg = PipelineConfig::new(SourceLang::Fixed(LangTag::sv()), LangTag::en());
    let r = rig_with(
        MockOcr::fixed(lines.clone()),
        MockTranslator::new(),
        cfg.clone(),
    );
    assert_eq!(cycle(&r, LEFT).await.blocks.len(), 1);

    cfg.low_confidence = LowConfidence::Dim;
    let r = rig_with(MockOcr::fixed(lines), MockTranslator::new(), cfg);
    let u = cycle(&r, LEFT).await;
    assert_eq!(u.blocks.len(), 2);
    assert!(!u.blocks[0].dim && u.blocks[1].dim);
}

#[tokio::test]
async fn priv07_debug_output_has_no_text() {
    let r = rig();
    let u = cycle(&r, LEFT).await;
    let dbg = format!("{u:?}");
    assert!(!dbg.contains("Spara") && !dbg.contains("[en]"), "{dbg}");
}

// ---------------------------------------------------------------- live driver

fn start(
    r: &Rig,
) -> (
    driver::DriverHandle,
    tokio::task::JoinHandle<pg_pipeline::DriverStats>,
    mpsc::UnboundedReceiver<LensUpdate>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let (h, j) = driver::spawn(r.pipeline.clone(), SchedulerConfig::default(), tx);
    (h, j, rx)
}

#[tokio::test(start_paused = true)]
async fn lens06_hidden_lens_runs_no_cycles() {
    let r = rig();
    let (h, j, _rx) = start(&r);
    h.lens_moved(LEFT);
    sleep(Duration::from_secs(10)).await;
    h.shutdown();
    let stats = j.await.unwrap();
    assert_eq!(stats.cycles, 0);
    assert_eq!(r.capturer.captures(), 0);
}

#[tokio::test(start_paused = true)]
async fn lens06_hide_parks_the_pipeline() {
    let r = rig();
    let (h, j, mut rx) = start(&r);
    h.lens_moved(LEFT);
    h.show();
    next_update(&mut rx).await;
    assert_eq!(r.capturer.releases(), 0);
    h.hide();
    sleep(Duration::from_millis(50)).await;
    let captures = r.capturer.captures();
    sleep(Duration::from_secs(10)).await;
    assert_eq!(r.capturer.captures(), captures, "no capture while hidden");
    assert_eq!(
        r.capturer.releases(),
        1,
        "hiding releases capture resources"
    );
    h.shutdown();
    j.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn cap03_rest_capture_then_unchanged_rechecks_skip_ocr() {
    let r = rig();
    let (h, j, mut rx) = start(&r);
    h.show();
    h.lens_moved(LEFT);
    let u = next_update(&mut rx).await;
    assert_eq!(u.blocks[0].source, SV_TEXT);
    sleep(Duration::from_secs(2)).await;
    assert_eq!(r.ocr.calls(), 1, "unchanged frames skip OCR");
    assert!(r.capturer.captures() >= 8, "re-checked at 4 Hz");
    h.shutdown();
    let stats = j.await.unwrap();
    assert!(stats.unchanged >= 7);
    assert_eq!(stats.emitted, 1);
}

#[tokio::test(start_paused = true)]
async fn cap03_no_ocr_while_moving() {
    let r = rig();
    let (h, j, _rx) = start(&r);
    h.show();
    for i in 0..40 {
        h.lens_moved(PhysRect::new(i * 10, 0, 600, 300));
        sleep(Duration::from_millis(30)).await;
    }
    assert_eq!(r.ocr.calls(), 0);
    h.shutdown();
    j.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn cap03_content_change_is_picked_up() {
    let r = rig();
    let (h, j, mut rx) = start(&r);
    h.show();
    h.lens_moved(LEFT);
    assert_eq!(next_update(&mut rx).await.blocks.len(), 1);
    r.capturer.set_desktop(desktop(1));
    let u = next_update(&mut rx).await;
    assert_eq!(u.blocks.len(), 2);
    assert_eq!(r.ocr.calls(), 2);
    h.shutdown();
    j.await.unwrap();
}

/// `desktop(0)` with a `w`×`h` rectangle at (`x`, `y`) filled with grey `v`.
fn desktop_with(x: usize, y: usize, w: usize, h: usize, v: u8) -> Frame {
    let mut f = desktop(0);
    let stride = f.stride();
    let data = f.data_mut();
    for row in y..y + h {
        for col in x..x + w {
            let i = row * stride + col * 4;
            data[i..i + 3].copy_from_slice(&[v, v, v]);
        }
    }
    f
}

async fn recheck(r: &Rig) -> CycleOutcome {
    let t = Tick {
        force: false,
        ..tick(LEFT)
    };
    r.pipeline.run_cycle(t).await
}

#[tokio::test]
async fn cap03_caret_blink_skips_ocr() {
    let r = rig();
    cycle(&r, LEFT).await;
    for i in 0..8 {
        // A 1×19 px caret after the text, on and off.
        let f = if i % 2 == 0 {
            desktop_with(330, 18, 1, 19, 0)
        } else {
            desktop(0)
        };
        r.capturer.set_desktop(f);
        assert!(matches!(recheck(&r).await, CycleOutcome::Unchanged), "{i}");
    }
    assert_eq!(r.ocr.calls(), 1);
}

#[tokio::test]
async fn cap03_small_changes_add_up() {
    // A panel darkens by 2 levels per re-check, below the cell tolerance each time. Compared with
    // the previous capture it would never count; compared with the processed frame it does.
    let r = rig();
    cycle(&r, LEFT).await;
    r.capturer
        .set_desktop(desktop_with(100, 150, 200, 100, 253));
    assert!(matches!(recheck(&r).await, CycleOutcome::Unchanged));
    r.capturer
        .set_desktop(desktop_with(100, 150, 200, 100, 251));
    assert!(matches!(recheck(&r).await, CycleOutcome::Update(_)));
    assert_eq!(r.ocr.calls(), 2);
}

#[tokio::test(start_paused = true)]
async fn cap03_stale_results_are_never_drawn() {
    let capturer = Shared(Arc::new(Mutex::new(StaticCapturer::new(desktop(0)))));
    let inner = Arc::new(MockTranslator::new());
    let pipeline = Arc::new(Pipeline::new(
        Box::new(capturer),
        Arc::new(ocr()),
        Arc::new(Slow(inner.clone(), Duration::from_millis(300))),
        PipelineConfig::new(SourceLang::Fixed(LangTag::sv()), LangTag::en()),
    ));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (h, j) = driver::spawn(pipeline, SchedulerConfig::default(), tx);
    h.show();
    h.lens_moved(LEFT);
    // At rest after 150 ms; translation is in flight until ~450 ms. Move away at 200 ms.
    sleep(Duration::from_millis(200)).await;
    h.lens_moved(RIGHT);
    let u = next_update(&mut rx).await;
    assert_eq!(
        u.blocks[0].source, EN_TEXT,
        "only the new position is drawn"
    );
    sleep(Duration::from_secs(1)).await;
    assert!(
        rx.try_recv().is_err(),
        "no late update for the old position"
    );
    h.shutdown();
    let stats = j.await.unwrap();
    assert_eq!(stats.cancelled, 1, "left cycle was in flight and dropped");
    assert_eq!(stats.emitted, 1);
    assert_eq!(
        inner.calls(),
        1,
        "the dropped cycle never reached the backend"
    );
}

#[tokio::test(start_paused = true)]
async fn tr04_moving_back_uses_cache() {
    let r = rig();
    let (h, j, mut rx) = start(&r);
    h.show();
    h.lens_moved(LEFT);
    next_update(&mut rx).await;
    h.lens_moved(RIGHT);
    next_update(&mut rx).await;
    let sent = r.translator.texts_sent();
    h.lens_moved(LEFT);
    let u = next_update(&mut rx).await;
    assert_eq!(u.blocks[0].source, SV_TEXT);
    assert_eq!(r.translator.texts_sent(), sent, "served from cache");
    h.shutdown();
    j.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn invalidate_after_language_change() {
    let r = rig();
    let (h, j, mut rx) = start(&r);
    h.show();
    h.lens_moved(LEFT);
    next_update(&mut rx).await;
    r.pipeline.set_config(PipelineConfig::new(
        SourceLang::Fixed(LangTag::en()),
        LangTag::sv(),
    ));
    h.invalidate();
    let u = next_update(&mut rx).await;
    assert_eq!(u.blocks[0].target_lang, LangTag::sv());
    h.shutdown();
    j.await.unwrap();
}
