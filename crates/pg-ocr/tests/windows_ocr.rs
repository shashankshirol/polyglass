//! Windows OCR against text rendered with an installed system font.
//!
//! The renderer draws grayscale anti-aliased Segoe UI, so these are smoke tests of the engine
//! wiring (text, boxes, formats, languages). The OCR-04/SV-03 accuracy gates on the fixture
//! images are in `accuracy.rs` (M1-OCR-6).
#![cfg(windows)]

use pg_core::{Error, Frame, LangTag, Line, PixelFormat, Rect};
use pg_ocr::{OcrEngine, WindowsOcr};
use pg_testkit::TextRenderer;
use pg_testkit::metrics::cer;

const LINES: [(&str, f32); 3] = [
    ("Save changes before closing?", 20.0),
    ("Your document has unsaved changes.", 16.0),
    ("Changes are saved every 5 minutes.", 14.0),
];

fn renderer() -> TextRenderer {
    TextRenderer::system().expect("Segoe UI or Arial is installed on Windows")
}

/// A 600×300 BGRA frame (the PERF-03 size) with `LINES`, and their rendered boxes.
fn english_frame(bg: [u8; 3], fg: [u8; 3]) -> (Frame, Vec<Rect>) {
    let r = renderer();
    // Frame::solid is RGBA; swap to BGRA order so the engine's BGRA path is the one tested.
    let solid = Frame::solid(600, 300, [bg[2], bg[1], bg[0]]);
    let mut frame = Frame::new(
        600,
        300,
        solid.stride(),
        PixelFormat::Bgra8,
        solid.data().to_vec(),
    )
    .unwrap();
    let mut top = 20.0;
    let boxes = LINES
        .iter()
        .map(|(text, px)| {
            let b = r.draw(&mut frame, 24.0, top, *px, text, fg);
            top += r.line_height(*px) * 2.0;
            b
        })
        .collect();
    (frame, boxes)
}

fn normalized(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn assert_matches_rendered(lines: &[Line], boxes: &[Rect]) {
    let texts: Vec<String> = lines.iter().map(|l| normalized(&l.text)).collect();
    assert_eq!(texts.len(), LINES.len(), "{texts:?}");
    for ((got, (want, _)), (line, rendered)) in texts.iter().zip(LINES).zip(lines.iter().zip(boxes))
    {
        // Typical misreads with this renderer are 5→S and rn→m: allow ~2 per line.
        assert!(cer(got, want) <= 0.06, "{got:?} vs {want:?}");
        assert_eq!(line.confidence, 1.0, "Windows OCR reports no confidence");
        // The union of word boxes covers the glyph ink, so it sits inside the rendered line box
        // (advance width × ascent−descent) give or take a few pixels.
        let b = line.bbox;
        assert!((b.x - rendered.x).abs() <= 3.0, "{b:?} vs {rendered:?}");
        assert!(
            (b.right() - rendered.right()).abs() <= 4.0,
            "{b:?} vs {rendered:?}"
        );
        assert!(
            b.vertical_overlap(rendered) >= b.h * 0.9,
            "{b:?} vs {rendered:?}"
        );
    }
}

#[test]
fn ocr01_recognizes_rendered_english_lines_with_boxes() {
    let ocr = WindowsOcr::new().unwrap();
    let (frame, boxes) = english_frame([255, 255, 255], [20, 20, 20]);
    let lines = ocr.recognize(&frame, Some(&LangTag::en())).unwrap();
    assert_matches_rendered(&lines, &boxes);
}

#[test]
fn ocr01_boxes_are_in_frame_pixels_at_any_upscale() {
    let (frame, boxes) = english_frame([255, 255, 255], [20, 20, 20]);
    for k in [1, 2, 3] {
        let ocr = WindowsOcr::new().unwrap().with_upscale(k);
        let lines = ocr.recognize(&frame, Some(&LangTag::en())).unwrap();
        // Text accuracy varies with k (1× drops words); the first, largest line is stable.
        let b = lines[0].bbox;
        assert!((b.x - boxes[0].x).abs() <= 3.0, "k={k}: {b:?}");
        assert!((b.right() - boxes[0].right()).abs() <= 4.0, "k={k}: {b:?}");
        assert!(b.vertical_overlap(&boxes[0]) >= b.h * 0.9, "k={k}: {b:?}");
    }
}

#[test]
fn ocr01_dark_theme_and_pixel_formats() {
    let ocr = WindowsOcr::new().unwrap();
    let (bgra, boxes) = english_frame([32, 32, 32], [230, 230, 230]);
    let lines = ocr.recognize(&bgra, Some(&LangTag::en())).unwrap();
    assert_matches_rendered(&lines, &boxes);

    // Same pixels as RGBA with a padded stride, and as Gray8.
    let (w, h) = (bgra.width(), bgra.height());
    let stride = w as usize * 4 + 8;
    let mut rgba = vec![0; stride * h as usize];
    let mut gray = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let [r, g, b] = bgra.rgb(x, y);
            let i = y as usize * stride + x as usize * 4;
            rgba[i..i + 4].copy_from_slice(&[r, g, b, 255]);
            gray.push(bgra.luma(x, y));
        }
    }
    let rgba = Frame::new(w, h, stride, PixelFormat::Rgba8, rgba).unwrap();
    let gray = Frame::new(w, h, w as usize, PixelFormat::Gray8, gray).unwrap();
    assert_matches_rendered(&ocr.recognize(&rgba, Some(&LangTag::en())).unwrap(), &boxes);
    assert_matches_rendered(&ocr.recognize(&gray, Some(&LangTag::en())).unwrap(), &boxes);
}

#[test]
fn blank_frame_has_no_lines() {
    let ocr = WindowsOcr::new().unwrap();
    let frame = Frame::solid(600, 300, [255, 255, 255]);
    assert!(ocr.recognize(&frame, None).unwrap().is_empty());
}

#[test]
fn ocr06_languages_are_queried_at_runtime() {
    let ocr = WindowsOcr::new().unwrap();
    let langs = ocr.supported_languages();
    assert_eq!(langs, WindowsOcr::available_languages().unwrap());
    // Every Windows install has the OCR language of its display language; this machine and CI
    // runners have English.
    assert!(langs.iter().any(|l| l.primary() == "en"), "{langs:?}");
}

/// SV-02: without the Swedish OCR feature, asking for Swedish must fail with
/// `OcrLanguageMissing` (the lens then explains how to install it); with it, Swedish text
/// including å/ä/ö is recognized. The test checks whichever branch this machine is on.
#[test]
fn sv02_swedish_missing_or_recognized() {
    let ocr = WindowsOcr::new().unwrap();
    let r = renderer();
    let mut frame = Frame::solid(600, 300, [255, 255, 255]);
    let text = "Vill du spara ändringarna i dokumentet för år 2026?";
    r.draw(&mut frame, 24.0, 40.0, 20.0, text, [20, 20, 20]);
    let installed = ocr.supported_languages().iter().any(LangTag::is_swedish);
    match ocr.recognize(&frame, Some(&LangTag::sv())) {
        Err(Error::OcrLanguageMissing(tag)) => {
            assert!(!installed, "sv is installed but reported missing");
            assert!(tag.is_swedish());
        }
        Ok(lines) => {
            assert!(installed, "sv recognized but not listed");
            assert_eq!(lines.len(), 1);
            let got = normalized(&lines[0].text);
            assert!(cer(&got, text) <= 0.04, "{got:?}");
            assert!(got.contains('ä') && got.contains('å'), "{got:?}");
        }
        Err(e) => panic!("unexpected error: {e}"),
    }
    // English keeps working meanwhile (SV-02).
    assert!(
        !ocr.recognize(&frame, Some(&LangTag::en()))
            .unwrap()
            .is_empty()
    );
}

/// OCR-03/SV-02 on the real engine regardless of what this machine has installed: Windows OCR
/// has no Klingon recognizer.
#[test]
fn ocr03_uninstalled_language_is_reported_missing() {
    let ocr = WindowsOcr::new().unwrap();
    let tlh = LangTag::parse("tlh").unwrap();
    let frame = Frame::solid(100, 50, [255, 255, 255]);
    match ocr.recognize(&frame, Some(&tlh)) {
        Err(Error::OcrLanguageMissing(tag)) => assert_eq!(tag, tlh),
        other => panic!("expected OcrLanguageMissing, got {other:?}"),
    }
}

#[test]
fn rejects_frames_over_the_engine_limit() {
    let ocr = WindowsOcr::new().unwrap();
    let frame = Frame::solid(20_000, 1, [255, 255, 255]);
    assert!(matches!(
        ocr.recognize(&frame, None),
        Err(Error::InvalidInput(_))
    ));
}
