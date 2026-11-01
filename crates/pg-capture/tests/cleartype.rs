//! OCR accuracy gates on screen text (M1-OCR-6): OCR-04 for English, SV-03 for Swedish.
//!
//! The fixture images (`pg-testkit`, `pg-ocr/tests/accuracy.rs`) are grayscale anti-aliased
//! without hinting. Real screen text is drawn by GDI/DirectWrite with hinting and ClearType. These
//! gates lay out every fixture the same way (`FixtureImage::render`: lines, font sizes,
//! positions), draw each lens-sized strip with GDI ClearType Segoe UI into a real window on the
//! primary monitor, capture it with WGC and score Windows OCR on it.
//!
//! Scales are simulated on the primary monitor: GDI draws at the scaled pixel size and the
//! captured frame is tagged with the scale, so OCR upscales as it would on such a monitor (GDI
//! output for a pixel size doesn't depend on the monitor's DPI). The windows briefly appear on
//! screen. Like `accuracy.rs`, the gates skip with a message when Segoe UI or an OCR language is
//! missing (`POLYGLASS_REQUIRE_OCR_GATES=1` fails instead). For the per-image table:
//!
//! ```text
//! cargo test -p pg-capture --release --test cleartype -- --nocapture
//! ```
#![cfg(windows)]
// Creating and painting test windows needs raw Win32 calls; test code only.
#![allow(unsafe_code)]

use std::sync::Mutex;
use std::time::{Duration, Instant};

use pg_capture::{ScreenCapturer, WgcCapturer, monitors};
use pg_core::{Frame, LangTag, PhysRect};
use pg_ocr::{OcrEngine, WindowsOcr};
use pg_testkit::TextRenderer;
use pg_testkit::fixture::{self, FixtureImage, MARGIN, SCALES, Theme};
use pg_testkit::metrics::{Score, score};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush,
    DEFAULT_CHARSET, DEFAULT_PITCH, DeleteObject, EndPaint, FW_NORMAL, FillRect,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, SelectObject, SetBkMode, SetTextAlign, SetTextColor,
    TA_BASELINE, TA_LEFT, TRANSPARENT, TextOutW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetWindowLongPtrW, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SW_SHOWNOACTIVATE,
    SetWindowLongPtrW, ShowWindow, TranslateMessage, WM_ERASEBKGND, WM_PAINT, WNDCLASSW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

/// The default lens height in logical px (SPEC §5), as in `accuracy.rs`.
const LENS_HEIGHT: f32 = 300.0;

/// The gates share the screen.
static SCREEN: Mutex<()> = Mutex::new(());

struct Line {
    text: Vec<u16>,
    /// Font size (em) in px, the size `TextRenderer` draws at.
    em: i32,
    x: i32,
    baseline: i32,
}

struct Page {
    bg: [u8; 3],
    fg: [u8; 3],
    lines: Vec<Line>,
}

fn colorref([r, g, b]: [u8; 3]) -> COLORREF {
    COLORREF(u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => unsafe {
            let page = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Page;
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if let Some(p) = page.as_ref() {
                let brush = CreateSolidBrush(colorref(p.bg));
                FillRect(hdc, &ps.rcPaint, brush);
                let _ = DeleteObject(brush.into());
                SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, colorref(p.fg));
                SetTextAlign(hdc, TA_BASELINE | TA_LEFT);
                for line in &p.lines {
                    // A negative height asks for the em size.
                    let font = CreateFontW(
                        -line.em,
                        0,
                        0,
                        0,
                        FW_NORMAL.0 as i32,
                        0,
                        0,
                        0,
                        DEFAULT_CHARSET,
                        OUT_DEFAULT_PRECIS,
                        CLIP_DEFAULT_PRECIS,
                        CLEARTYPE_QUALITY,
                        DEFAULT_PITCH.0 as u32,
                        w!("Segoe UI"),
                    );
                    let old = SelectObject(hdc, font.into());
                    let _ = TextOutW(hdc, line.x, line.baseline, &line.text);
                    SelectObject(hdc, old);
                    let _ = DeleteObject(font.into());
                }
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        },
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// A topmost popup at a physical rect showing `page`.
struct PageWindow {
    hwnd: HWND,
}

impl PageWindow {
    fn open(rect: PhysRect, page: Page) -> Self {
        unsafe {
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(None).unwrap().into();
            let class = w!("PgCaptureClearTypeWindow");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc); // Fails harmlessly when already registered.
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                PCWSTR::null(),
                WS_POPUP,
                rect.x,
                rect.y,
                rect.width as i32,
                rect.height as i32,
                None,
                None,
                Some(instance),
                None,
            )
            .unwrap();
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(Box::new(page)) as isize);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            pump(Duration::from_millis(200));
            PageWindow { hwnd }
        }
    }
}

impl Drop for PageWindow {
    fn drop(&mut self) {
        unsafe {
            let page = GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *mut Page;
            let _ = DestroyWindow(self.hwnd);
            if !page.is_null() {
                drop(Box::from_raw(page));
            }
        }
        pump(Duration::from_millis(30));
    }
}

fn pump(d: Duration) {
    let end = Instant::now() + d;
    while Instant::now() < end {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Share of pixels not equal to `bg`: 0 until the window has been painted and composed.
fn ink(frame: &Frame, bg: [u8; 3]) -> f64 {
    let mut n = 0u32;
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            if frame.rgb(x, y) != bg {
                n += 1;
            }
        }
    }
    f64::from(n) / f64::from(frame.width() * frame.height())
}

/// Draw one strip of `img` with GDI ClearType, capture it and score Windows OCR on it.
fn read_band(
    cap: &mut WgcCapturer,
    ocr: &WindowsOcr,
    r: &TextRenderer,
    img: &FixtureImage,
    band: &fixture::Band,
    lang: &LangTag,
    at: (i32, i32),
) -> Score {
    let (bg, fg) = img.theme.colors();
    let s = img.scale as f32;
    let lines = img.lines[band.lines.clone()]
        .iter()
        .map(|l| {
            let px = l.font_px * s;
            Line {
                text: l.text.encode_utf16().collect(),
                em: px.round() as i32,
                x: (MARGIN * s).round() as i32,
                baseline: (l.bbox.y + r.ascent(px)).round() as i32 - band.y as i32,
            }
        })
        .collect();
    // GDI's hinted advances are wider than the layout's at small sizes: room so no line is cut.
    let rect = PhysRect::new(at.0, at.1, img.frame.width() * 5 / 4, band.height);
    let _w = PageWindow::open(rect, Page { bg, fg, lines });
    let end = Instant::now() + Duration::from_secs(2);
    let mut frame = cap.capture(rect).unwrap();
    while ink(&frame, bg) == 0.0 && Instant::now() < end {
        pump(Duration::from_millis(30));
        frame = cap.capture(rect).unwrap();
    }
    // Tagged with the simulated scale so OCR upscales as it would on such a monitor.
    let frame = frame.with_origin(rect, img.scale);
    let got: Vec<String> = ocr
        .recognize(&frame, Some(lang))
        .unwrap()
        .into_iter()
        .map(|l| l.text)
        .collect();
    let want = img.text_of(band.lines.clone());
    let s = score(&got.join("\n"), &want);
    if s.swedish.errors > 0 {
        eprintln!(
            "--- {} y={}\n  got:  {got:?}\n  want: {want:?}",
            img.stem(),
            band.y
        );
    }
    s
}

/// Skip (or fail, with `POLYGLASS_REQUIRE_OCR_GATES=1`) because `why`.
fn skip(why: &str) {
    if std::env::var_os("POLYGLASS_REQUIRE_OCR_GATES").is_some_and(|v| v == "1") {
        panic!("OCR accuracy gate can't run: {why}");
    }
    eprintln!("skipped: {why}");
}

/// Score every fixture of the Swedish or the English set drawn as screen text; `None` if skipped.
fn measure(swedish: bool) -> Option<Score> {
    let _screen = SCREEN.lock().unwrap_or_else(|e| e.into_inner());
    let Some(r) = TextRenderer::system().filter(|r| r.font_path().ends_with("segoeui.ttf")) else {
        skip("Segoe UI is not installed");
        return None;
    };
    let ocr = WindowsOcr::new().unwrap();
    let installed = ocr.supported_languages();
    let mut cap = WgcCapturer::new().unwrap();
    let primary = monitors()
        .unwrap()
        .into_iter()
        .find(|m| m.rect.x == 0 && m.rect.y == 0)
        .expect("a primary monitor at (0, 0)");
    let at = (primary.rect.x + 40, primary.rect.y + 40);

    let mut total = Score::default();
    let mut slices: Vec<((f64, Theme), Score)> = Vec::new();
    for truth in fixture::all()
        .unwrap()
        .iter()
        .filter(|t| t.is_swedish() == swedish)
    {
        let lang = truth.ocr_lang();
        if !installed.iter().any(|l| l.same_language(&lang)) {
            skip(&format!("Windows OCR language `{lang}` is not installed"));
            return None;
        }
        for scale in SCALES {
            for theme in Theme::ALL {
                let img = FixtureImage::render(&r, truth, scale, theme);
                let max_height = (LENS_HEIGHT * img.scale as f32) as u32;
                if primary.rect.width < img.frame.width() * 5 / 4 + 40
                    || primary.rect.height < max_height * 2
                {
                    skip("the primary monitor is too small for a 200% page");
                    return None;
                }
                let mut s = Score::default();
                for band in img.bands(max_height) {
                    s += &read_band(&mut cap, &ocr, &r, &img, &band, &lang, at);
                }
                eprintln!("{:<32} {s}", img.stem());
                total += &s;
                match slices.iter_mut().find(|(k, _)| *k == (scale, theme)) {
                    Some((_, acc)) => *acc += &s,
                    None => slices.push(((scale, theme), s)),
                }
            }
        }
    }
    for ((scale, theme), s) in &slices {
        eprintln!("{:>3}% {:<5}  {s}", scale * 100.0, theme.name());
    }
    eprintln!("set (GDI ClearType + WGC): {total}");
    Some(total)
}

/// OCR-04: CER ≤ 3% for Windows OCR on the English fixture set drawn as screen text.
#[test]
fn ocr04_english_screen_text_cer() {
    let Some(total) = measure(false) else { return };
    assert!(total.cer() <= 0.03, "English set: {total}");
}

/// SV-03: CER ≤ 3%, ≤ 2% errors on å ä ö Å Ä Ö and no misreading of one of those letters that
/// repeats (3 or more times across the set), on the Swedish fixture set (including
/// `mixed-sv-en`, read with the Swedish recognizer as Auto mode does) drawn as screen text.
#[test]
fn sv03_swedish_screen_text() {
    let Some(total) = measure(true) else { return };
    assert!(total.cer() <= 0.03, "Swedish set: {total}");
    assert!(total.swedish.value() <= 0.02, "Swedish set: {total}");
    assert!(total.systematic(3).is_empty(), "Swedish set: {total}");
}
