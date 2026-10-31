//! CAP-04 fixture tests: the lens region over fixture images at 100/125/150/200% (M1-CAP-5).
//!
//! `pg-testkit` renders a fixture page at each scale (M0-FIX-2) and `FixtureCapturer` serves it
//! as a monitor of that scale. The lens is placed in logical px over one line of text and
//! converted with `LogicalRect::to_physical`, as the app does; the captured frame must hold all
//! of that line's ink and none of its neighbours'. On Windows, Windows OCR then reads the line.
//! Skipped where no system UI font is installed.
#![cfg(feature = "fixture")]

use pg_capture::{FixtureCapturer, ScreenCapturer};
use pg_core::{Frame, LogicalRect, PhysRect};
use pg_testkit::TextRenderer;
use pg_testkit::fixture::{self, FixtureImage, RenderedLine, SCALES, Theme};

/// Pixels that differ from the background.
fn ink(frame: &Frame, bg: [u8; 3]) -> usize {
    (0..frame.height())
        .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| frame.rgb(x, y) != bg)
        .count()
}

/// Lines with at least 2 physical px of free space above and below, so a lens can hold one
/// line alone.
fn separate_lines(img: &FixtureImage) -> Vec<(&RenderedLine, f32, f32)> {
    let h = img.frame.height() as f32;
    img.lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let above = i.checked_sub(1).map_or(0.0, |j| img.lines[j].bbox.bottom());
            let below = img.lines.get(i + 1).map_or(h, |l| l.bbox.y);
            (line.bbox.y - above >= 2.0 && below - line.bbox.bottom() >= 2.0)
                .then_some((line, above, below))
        })
        .collect()
}

/// A lens in logical px around `line`, reaching a third of the way to its neighbours.
fn lens_over(line: &RenderedLine, above: f32, below: f32, scale: f64) -> LogicalRect {
    let s = scale as f32;
    let b = line.bbox;
    let top = b.y - (b.y - above) / 3.0;
    let bottom = b.bottom() + (below - b.bottom()) / 3.0;
    LogicalRect::new(
        f64::from(b.x / s - 4.0),
        f64::from(top / s),
        f64::from(b.w / s + 8.0),
        f64::from((bottom - top) / s),
    )
}

#[test]
fn cap04_lens_over_a_line_at_every_scale() {
    let Some(r) = TextRenderer::system() else {
        eprintln!("skipped: no system UI font");
        return;
    };
    let truth = fixture::load("ui-dialog-en").unwrap();
    for scale in SCALES {
        for theme in Theme::ALL {
            let img = FixtureImage::render(&r, &truth, scale, theme);
            let (w, h) = (img.frame.width(), img.frame.height());
            let mut cap = FixtureCapturer::from_png(&img.png(), scale).unwrap();
            assert_eq!(cap.bounds(), PhysRect::new(0, 0, w, h));
            let bg = theme.colors().0;
            let lines = separate_lines(&img);
            assert!(lines.len() >= 6, "{}: {}", img.stem(), lines.len());
            for (line, above, below) in lines {
                let region = lens_over(line, above, below, scale).to_physical(scale);
                let frame = cap.capture(region).unwrap();
                assert_eq!(frame.origin, region, "{}", img.stem());
                assert_eq!(frame.scale_factor, scale);
                // Everything between the neighbours' boxes is this line's ink.
                let zone_top = above.ceil() as u32;
                let zone = img
                    .frame
                    .crop(0, zone_top, w, below.floor() as u32 - zone_top);
                let want = ink(&zone, bg);
                assert!(want > 0);
                assert_eq!(ink(&frame, bg), want, "{} {:?}", img.stem(), line.text);
            }
        }
    }
}

/// A 100 logical px tall lens moved down the page at each scale (so its edges fall on
/// fractional physical pixels at 125% and 150%), read by Windows OCR. The CER bound is looser
/// than OCR-04's because Windows OCR drops some isolated short words ("Save") at 100%; accuracy
/// itself is gated in `pg-ocr/tests/accuracy.rs`.
#[cfg(windows)]
#[test]
fn cap04_windows_ocr_reads_the_lens_at_every_scale() {
    use pg_core::LangTag;
    use pg_ocr::{OcrEngine, WindowsOcr};
    use pg_testkit::metrics::{Score, score};

    let Some(r) = TextRenderer::system() else {
        eprintln!("skipped: no system UI font");
        return;
    };
    let ocr = WindowsOcr::new().unwrap();
    if !ocr
        .supported_languages()
        .iter()
        .any(|l| l.primary() == "en")
    {
        eprintln!("skipped: no English Windows OCR language installed");
        return;
    }
    let truth = fixture::load("ui-dialog-en").unwrap();
    for scale in SCALES {
        for theme in Theme::ALL {
            let img = FixtureImage::render(&r, &truth, scale, theme);
            let mut cap = FixtureCapturer::from_png(&img.png(), scale).unwrap();
            let s = scale as f32;
            let mut total = Score::default();
            for band in img.bands((100.0 * s) as u32) {
                let lens = LogicalRect::new(
                    0.0,
                    f64::from(band.y as f32 / s),
                    600.0,
                    f64::from(band.height as f32 / s),
                );
                let frame = cap.capture(lens.to_physical(scale)).unwrap();
                let got: Vec<String> = ocr
                    .recognize(&frame, Some(&LangTag::en()))
                    .unwrap()
                    .into_iter()
                    .map(|l| l.text)
                    .collect();
                total += &score(
                    &got.join(
                        "
",
                    ),
                    &img.text_of(band.lines),
                );
            }
            eprintln!("{}: {total}", img.stem());
            assert!(total.cer() <= 0.05, "{}: {total}", img.stem());
        }
    }
}
