//! Windows Graphics Capture against real on-screen windows (CAP-01, CAP-02, CAP-04).
//!
//! Each test opens small topmost, non-activating popup windows painted with a known colour or
//! text, captures over them, and closes them. They briefly appear on screen. Tests run one at a
//! time (`SCREEN`) because they share the screen.
#![cfg(windows)]
// Creating and painting test windows needs raw Win32 calls; test code only.
#![allow(unsafe_code)]

use std::sync::Mutex;
use std::time::{Duration, Instant};

use pg_capture::{ScreenCapturer, WgcCapturer, monitors};
use pg_core::{Frame, LangTag, PhysRect};
use pg_ocr::{OcrEngine, WindowsOcr};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush,
    DEFAULT_CHARSET, DEFAULT_PITCH, DeleteObject, EndPaint, FW_NORMAL, FillRect,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, TRANSPARENT, TextOutW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetWindowLongPtrW, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SW_SHOWNOACTIVATE,
    SetWindowDisplayAffinity, SetWindowLongPtrW, ShowWindow, TranslateMessage,
    WDA_EXCLUDEFROMCAPTURE, WDA_NONE, WM_ERASEBKGND, WM_PAINT, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

static SCREEN: Mutex<()> = Mutex::new(());

const MAGENTA: [u8; 3] = [255, 0, 255];
const CYAN: [u8; 3] = [0, 255, 255];

struct Paint {
    rgb: [u8; 3],
    text: Vec<u16>,
}

/// A topmost popup filled with one colour, optionally with black text, at a physical rect.
struct TestWindow {
    hwnd: HWND,
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => unsafe {
            let paint = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Paint;
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if let Some(p) = paint.as_ref() {
                let [r, g, b] = p.rgb;
                let brush = CreateSolidBrush(COLORREF(
                    u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16,
                ));
                FillRect(hdc, &ps.rcPaint, brush);
                let _ = DeleteObject(brush.into());
                if !p.text.is_empty() {
                    let font = CreateFontW(
                        -28,
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
                    SetBkMode(hdc, TRANSPARENT);
                    SetTextColor(hdc, COLORREF(0));
                    let _ = TextOutW(hdc, 16, 16, &p.text);
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

impl TestWindow {
    fn open(rect: PhysRect, rgb: [u8; 3], text: &str, exclude: bool) -> Self {
        unsafe {
            // Physical coordinates whatever the test process's DPI awareness.
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(None).unwrap().into();
            let class = w!("PgCaptureTestWindow");
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
            let paint = Box::new(Paint {
                rgb,
                text: text.encode_utf16().collect(),
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(paint) as isize);
            if exclude {
                SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).unwrap();
            }
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                hwnd,
                None,
                rect.x,
                rect.y,
                rect.width as i32,
                rect.height as i32,
                windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE
                    | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER,
            );
            let w = TestWindow { hwnd };
            pump(Duration::from_millis(250));
            w
        }
    }

    fn set_excluded(&self, exclude: bool) {
        let affinity = if exclude {
            WDA_EXCLUDEFROMCAPTURE
        } else {
            WDA_NONE
        };
        unsafe { SetWindowDisplayAffinity(self.hwnd, affinity).unwrap() };
        pump(Duration::from_millis(150));
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        unsafe {
            let paint = GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *mut Paint;
            let _ = DestroyWindow(self.hwnd);
            if !paint.is_null() {
                drop(Box::from_raw(paint));
            }
        }
        pump(Duration::from_millis(50));
    }
}

/// Run this thread's message loop for `d` so windows paint and DWM composes them.
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

fn capturer() -> WgcCapturer {
    WgcCapturer::new().expect("Windows Graphics Capture is available on the dev machine")
}

/// Fraction of pixels in columns `x0..x1` of `frame` that are close to `rgb`.
fn share(frame: &Frame, x0: u32, x1: u32, rgb: [u8; 3]) -> f64 {
    let mut hits = 0u32;
    for y in 0..frame.height() {
        for x in x0..x1 {
            let p = frame.rgb(x, y);
            if p.iter().zip(rgb).all(|(a, b)| a.abs_diff(b) <= 8) {
                hits += 1;
            }
        }
    }
    f64::from(hits) / f64::from((x1 - x0) * frame.height())
}

/// Capture until `ok(frame)` or a second passes (WGC delivers frames after the screen changes).
fn capture_until(cap: &mut WgcCapturer, region: PhysRect, ok: impl Fn(&Frame) -> bool) -> Frame {
    let end = Instant::now() + Duration::from_secs(1);
    loop {
        let f = cap.capture(region).unwrap();
        if ok(&f) || Instant::now() > end {
            return f;
        }
        pump(Duration::from_millis(30));
    }
}

#[test]
fn cap01_captures_exactly_the_region() {
    let _screen = SCREEN.lock().unwrap_or_else(|e| e.into_inner());
    let mut cap = capturer();
    let win = PhysRect::new(200, 200, 300, 150);
    let _w = TestWindow::open(win, MAGENTA, "", false);

    let inside = PhysRect::new(250, 220, 100, 50);
    let f = capture_until(&mut cap, inside, |f| share(f, 0, 100, MAGENTA) == 1.0);
    assert_eq!((f.width(), f.height()), (100, 50));
    assert_eq!(f.origin, inside);
    assert_eq!(f.scale_factor, 1.0);
    assert_eq!(share(&f, 0, 100, MAGENTA), 1.0);

    // Straddling the window's left edge at x = 200: exactly the right half is the window.
    let edge = PhysRect::new(150, 220, 100, 50);
    let f = capture_until(&mut cap, edge, |f| share(f, 50, 100, MAGENTA) == 1.0);
    assert_eq!(share(&f, 50, 100, MAGENTA), 1.0);
    assert!(
        share(&f, 49, 50, MAGENTA) < 1.0,
        "column left of the edge is not the window"
    );
}

#[test]
fn cap04_every_monitor_and_a_region_across_two() {
    let _screen = SCREEN.lock().unwrap_or_else(|e| e.into_inner());
    let mut cap = capturer();
    let mons = monitors().unwrap();
    assert!(!mons.is_empty());
    for m in &mons {
        let win = PhysRect::new(m.rect.x + 100, m.rect.y + 100, 240, 120);
        let _w = TestWindow::open(win, MAGENTA, "", false);
        let region = PhysRect::new(win.x + 20, win.y + 20, 200, 80);
        let f = capture_until(&mut cap, region, |f| share(f, 0, 200, MAGENTA) == 1.0);
        assert_eq!(share(&f, 0, 200, MAGENTA), 1.0, "monitor {:?}", m.rect);
        assert_eq!(f.scale_factor, m.scale_factor);
    }

    // Two monitors sharing a vertical edge (on the dev machine: x = 0 between the 150%
    // monitor on the left and the 100% primary): one window on each side.
    let pair = mons.iter().find_map(|left| {
        mons.iter().find_map(|right| {
            let x = left.rect.right();
            let y0 = i64::from(left.rect.y.max(right.rect.y));
            let y1 = left.rect.bottom().min(right.rect.bottom());
            (i64::from(right.rect.x) == x && y1 - y0 >= 400).then_some((x as i32, y0 as i32 + 200))
        })
    });
    let Some((x, y)) = pair else {
        eprintln!("skipped the cross-monitor part: no two side-by-side monitors");
        return;
    };
    let _a = TestWindow::open(PhysRect::new(x - 150, y, 150, 100), MAGENTA, "", false);
    let _b = TestWindow::open(PhysRect::new(x, y, 150, 100), CYAN, "", false);
    let region = PhysRect::new(x - 100, y + 20, 200, 40);
    let f = capture_until(&mut cap, region, |f| {
        share(f, 0, 100, MAGENTA) == 1.0 && share(f, 100, 200, CYAN) == 1.0
    });
    assert_eq!(share(&f, 0, 100, MAGENTA), 1.0);
    assert_eq!(share(&f, 100, 200, CYAN), 1.0);
}

#[test]
fn cap02_excluded_window_is_not_captured() {
    let _screen = SCREEN.lock().unwrap_or_else(|e| e.into_inner());
    let mut cap = capturer();
    let win = PhysRect::new(300, 300, 200, 100);
    let w = TestWindow::open(win, MAGENTA, "", false);
    let region = PhysRect::new(320, 320, 160, 60);
    let f = capture_until(&mut cap, region, |f| share(f, 0, 160, MAGENTA) == 1.0);
    assert_eq!(share(&f, 0, 160, MAGENTA), 1.0, "visible before exclusion");

    w.set_excluded(true);
    let f = capture_until(&mut cap, region, |f| share(f, 0, 160, MAGENTA) == 0.0);
    assert_eq!(
        share(&f, 0, 160, MAGENTA),
        0.0,
        "excluded window still captured"
    );
}

/// CAP-02 marker test: a window that excludes itself draws a marker string; OCR over 100
/// capture cycles never sees it. A control run without exclusion must see it, which shows the
/// test can detect a leak.
#[test]
fn cap02_marker_is_never_recognized_over_100_cycles() {
    let _screen = SCREEN.lock().unwrap_or_else(|e| e.into_inner());
    let mut cap = capturer();
    let ocr = WindowsOcr::new().unwrap();
    let win = PhysRect::new(260, 420, 520, 80);
    let region = PhysRect::new(win.x, win.y, win.width, win.height);
    let marker = "POLYGLASS MARKER QX7";
    let seen = |f: &Frame| {
        ocr.recognize(f, Some(&LangTag::en()))
            .unwrap()
            .iter()
            .any(|l| l.text.to_uppercase().contains("MARKER"))
    };

    {
        let _control = TestWindow::open(win, [255, 255, 255], marker, false);
        let f = capture_until(&mut cap, region, seen);
        assert!(
            seen(&f),
            "control: the marker must be readable when not excluded"
        );
    }
    pump(Duration::from_millis(200));

    let _lens = TestWindow::open(win, [255, 255, 255], marker, true);
    for cycle in 0..100 {
        let f = cap.capture(region).unwrap();
        assert!(!seen(&f), "marker recognized in cycle {cycle}");
        pump(Duration::from_millis(5));
    }
}

#[test]
fn release_closes_sessions_and_capture_reopens_them() {
    let _screen = SCREEN.lock().unwrap_or_else(|e| e.into_inner());
    let mut cap = capturer();
    let region = PhysRect::new(10, 10, 64, 64);
    cap.capture(region).unwrap();
    assert_eq!(cap.active_sessions(), 1);
    cap.release();
    assert_eq!(cap.active_sessions(), 0);
    cap.capture(region).unwrap();
    assert_eq!(cap.active_sessions(), 1);
}

#[test]
fn region_outside_every_monitor_is_an_error() {
    let mut cap = capturer();
    assert!(
        cap.capture(PhysRect::new(1_000_000, 1_000_000, 10, 10))
            .is_err()
    );
    assert!(cap.capture(PhysRect::new(0, 0, 0, 10)).is_err());
}

/// `cargo test -p pg-capture --release --test wgc -- --ignored --nocapture`
#[test]
#[ignore = "measurement"]
fn measure_capture_latency() {
    let _screen = SCREEN.lock().unwrap_or_else(|e| e.into_inner());
    let mut cap = capturer();
    for m in monitors().unwrap() {
        let region = PhysRect::new(m.rect.x + 100, m.rect.y + 100, 600, 300);
        let t0 = Instant::now();
        cap.capture(region).unwrap();
        let first = t0.elapsed();
        let mut v: Vec<Duration> = (0..100)
            .map(|_| {
                let t = Instant::now();
                cap.capture(region).unwrap();
                t.elapsed()
            })
            .collect();
        v.sort();
        eprintln!(
            "monitor {:?} @{}: first {:.1} ms, p50 {:.2} ms, p95 {:.2} ms",
            m.rect,
            m.scale_factor,
            first.as_secs_f64() * 1e3,
            v[49].as_secs_f64() * 1e3,
            v[94].as_secs_f64() * 1e3
        );
    }
}
