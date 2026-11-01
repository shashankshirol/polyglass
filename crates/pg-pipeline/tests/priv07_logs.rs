//! PRIV-07: the pipeline logs no recognized or translated text, not even at TRACE (stricter than
//! the default level the app runs at, and what `--trace-timings` adds to).
//!
//! A test binary of its own: `tracing` caches per call site whether anyone listens, so another
//! test hitting the same call site without a subscriber would switch these logs off.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use pg_capture::StaticCapturer;
use pg_core::{Frame, LangTag, Line, PhysRect, Rect, SourceLang};
use pg_ocr::MockOcr;
use pg_pipeline::{CycleOutcome, Pipeline, PipelineConfig, SchedulerConfig, Tick, driver};
use pg_translate::{MockTranslator, TranslateError};
use tokio::sync::mpsc;

const SV_TEXT: &str = "Spara dina ändringar innan du stänger fönstret";
const EN_TEXT: &str = "Your changes have been saved to the cloud";
const REGION: PhysRect = PhysRect::new(0, 0, 600, 300);

#[derive(Clone, Default)]
struct LogSink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// White page with a dark bar, read as one Swedish and one English line; Auto mode translates
/// the first and passes the second through.
fn pipeline(translator: MockTranslator) -> Arc<Pipeline> {
    let mut page = Frame::solid(600, 300, [255, 255, 255]);
    let stride = page.stride();
    for y in 20..34 {
        page.data_mut()[y * stride + 80..y * stride + 1200].fill(30);
    }
    let ocr = MockOcr::fixed(vec![
        Line::new(SV_TEXT, Rect::new(20.0, 20.0, 300.0, 14.0), 0.95),
        Line::new(EN_TEXT, Rect::new(20.0, 120.0, 300.0, 14.0), 0.95),
    ]);
    Arc::new(Pipeline::new(
        Box::new(StaticCapturer::new(page)),
        Arc::new(ocr),
        Arc::new(translator),
        PipelineConfig::new(SourceLang::AutoSvEn, LangTag::en()),
    ))
}

#[test]
fn priv07_logs_have_no_text() {
    let sink = LogSink::default();
    let writer = sink.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let tick = Tick {
                generation: 1,
                region: REGION,
                force: true,
            };
            // A translated block and a passed-through one.
            let p = pipeline(MockTranslator::new());
            let CycleOutcome::Update(u) = p.run_cycle(tick).await else {
                panic!("expected an update");
            };
            assert_eq!(u.blocks.len(), 2);
            // A backend error that quotes the text.
            let failing = MockTranslator::new();
            failing.set_failure(Some(TranslateError::Backend(SV_TEXT.into())));
            pipeline(failing).run_cycle(tick).await;
            // The driver.
            let (tx, mut rx) = mpsc::unbounded_channel();
            let (h, task) = driver::spawn(
                pipeline(MockTranslator::new()),
                SchedulerConfig::default(),
                tx,
            );
            h.lens_moved(REGION);
            h.show();
            tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .expect("an update within 5 s")
                .expect("driver alive");
            h.shutdown();
            task.await.unwrap();
        });
    });
    let logs = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("cycle"), "the timings are logged: {logs}");
    for text in [
        SV_TEXT,
        EN_TEXT,
        "Spara",
        "ändringar",
        "[en]",
        "Your changes",
    ] {
        assert!(!logs.contains(text), "{text:?} in the logs:\n{logs}");
    }
}
