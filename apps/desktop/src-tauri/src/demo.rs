//! `--demo`: mock capture/OCR/translation so the lens → pipeline → UI flow is visible end to end
//! before real engines land (M1-CAP-1, M1-OCR-1, M1-TR-1). The UI shows a DEMO badge while this
//! is active, and nothing here reads the real screen.

use std::sync::Arc;
use std::time::Duration;

use pg_capture::StaticCapturer;
use pg_core::{Frame, LangTag, Line, PhysRect, Rect, SourceLang};
use pg_ocr::MockOcr;
use pg_pipeline::{Pipeline, PipelineConfig};
use pg_translate::MockTranslator;

/// Swedish UI strings with their mock translations, plus English lines that pass through
/// untranslated (TR-07).
const PHRASES: &[(&str, Option<&str>)] = &[
    ("Spara ändringar", Some("Save changes")),
    (
        "Vill du spara ändringarna innan du stänger?",
        Some("Do you want to save the changes before closing?"),
    ),
    ("Avbryt", Some("Cancel")),
    ("Inställningar för sekretess", Some("Privacy settings")),
    ("Your changes have been saved", None),
    (
        "Filen kunde inte öppnas",
        Some("The file could not be opened"),
    ),
    ("Öppna mapp", Some("Open folder")),
    (
        "Senast ändrad: 3 oktober 2026",
        Some("Last modified: 3 October 2026"),
    ),
];

/// Build a pipeline over a synthetic desktop covering `bounds` (all monitors, physical px).
pub fn pipeline(bounds: PhysRect, scale: f64) -> Pipeline {
    let translator = PHRASES
        .iter()
        .filter_map(|(sv, en)| en.map(|en| (*sv, en)))
        .fold(MockTranslator::new().with_id("demo"), |t, (sv, en)| {
            t.with_entry(sv, en)
        });
    let ocr = MockOcr::with_fn(read_phrases).with_delay(Duration::from_millis(80));
    Pipeline::new(
        Box::new(StaticCapturer::new(desktop(bounds, scale))),
        Arc::new(ocr),
        Arc::new(translator),
        PipelineConfig::new(SourceLang::AutoSvEn, LangTag::en()),
    )
}

/// Light grey "page" with darker text-like bars every 32 px so the change detector never sees
/// a blank region.
fn desktop(bounds: PhysRect, scale: f64) -> Frame {
    let mut f = Frame::solid(bounds.width, bounds.height, [236, 238, 242]);
    let stride = f.stride();
    let (w, h) = (bounds.width as usize, bounds.height as usize);
    let data = f.data_mut();
    for y in (8..h).step_by(32) {
        for row in y..(y + 12).min(h) {
            for x in (0..w).filter(|x| x % 240 < 200) {
                let i = row * stride + x * 4;
                data[i..i + 3].copy_from_slice(&[60, 64, 72]);
            }
        }
    }
    f.with_origin(bounds, scale)
}

/// Mock OCR: up to three lines whose text depends on where the frame was captured, so moving
/// the lens visibly changes the result.
fn read_phrases(frame: &Frame) -> Vec<Line> {
    let s = frame.scale_factor as f32;
    let (w, h) = (frame.width() as f32, frame.height() as f32);
    let seed = (frame.origin.x.unsigned_abs() / 160 + frame.origin.y.unsigned_abs() / 90) as usize;
    let line_h = 18.0 * s;
    // Far enough apart that block grouping keeps each line as its own block.
    let step = 40.0 * s;
    (0..3)
        .map_while(|i| {
            let y = 8.0 * s + i as f32 * step;
            (y + line_h <= h).then_some((i, y))
        })
        .map(|(i, y)| {
            let text = PHRASES[(seed + i * 3) % PHRASES.len()].0;
            let width = (text.chars().count() as f32 * 7.5 * s).min(w - 16.0 * s);
            Line::new(text, Rect::new(8.0 * s, y, width.max(1.0), line_h), 0.92)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pg_pipeline::{CycleOutcome, LensStatus, Tick};

    #[tokio::test]
    async fn demo_pipeline_translates_swedish_and_passes_english_through() {
        let p = pipeline(PhysRect::new(-200, 0, 1200, 600), 1.25);
        let tick = Tick {
            generation: 1,
            region: PhysRect::new(0, 0, 500, 200),
            force: true,
        };
        let CycleOutcome::Update(u) = p.run_cycle(tick).await else {
            panic!("expected an update");
        };
        assert_eq!(u.status, LensStatus::Ok);
        assert!(u.backend.local, "PRIV-05 indicator must show on-device");
        assert!(!u.blocks.is_empty());
        for b in &u.blocks {
            assert!(b.translation.is_some());
            assert!(b.bg.starts_with('#') && b.fg.starts_with('#'));
        }
    }

    #[test]
    fn mock_ocr_fits_lines_inside_small_lenses() {
        let f = Frame::solid(108, 12, [255, 255, 255]);
        assert!(read_phrases(&f).is_empty());
        let f = Frame::solid(300, 70, [255, 255, 255]);
        let lines = read_phrases(&f);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| l.bbox.right() <= 300.0));
    }
}
