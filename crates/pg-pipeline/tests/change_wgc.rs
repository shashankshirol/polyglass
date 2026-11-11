//! Change detection (CAP-03) on real screen captures.
//!
//! The unit tests in `src/change.rs` use synthetic frames. These draw GDI ClearType text into a
//! real topmost window, capture it with Windows Graphics Capture and compare the frames the way
//! the pipeline does: a blinking caret must not count as a change; an edited character, typed
//! text, a scroll and video must. Each pair of states is scored at several alignments of the
//! 8 px cell grid (the lens can sit anywhere; the table uses all 64) by cropping one larger
//! capture.
//!
//! Scales are simulated on the primary monitor (text, caret and lens drawn at the scaled pixel
//! size), as in `pg-capture/tests/cleartype.rs`. The windows briefly appear on screen and need a
//! desktop session with WGC. For the measurement table:
//!
//! ```text
//! cargo test -p pg-pipeline --release --test change_wgc -- --ignored --nocapture
//! ```
#![cfg(windows)]
// Creating and painting test windows needs raw Win32 calls; test code only.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use pg_capture::{ScreenCapturer, WgcCapturer, monitors};
use pg_core::{Frame, PhysRect};
use pg_pipeline::change::{ChangeConfig, FrameSignature};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush,
    DEFAULT_CHARSET, DEFAULT_PITCH, DeleteObject, EndPaint, FW_NORMAL, FillRect, InvalidateRect,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, SelectObject, SetBkMode, SetTextAlign, SetTextColor,
    TA_BASELINE, TA_LEFT, TRANSPARENT, TextOutW, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
    RegisterClassW, SW_SHOWNOACTIVATE, ShowWindow, TranslateMessage, WM_ERASEBKGND, WM_PAINT,
    WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

/// The tests share the screen.
static SCREEN: Mutex<()> = Mutex::new(());

/// The default lens size in logical px (SPEC §5).
const LENS: (u32, u32) = (600, 300);

const WHITE: [u8; 3] = [255, 255, 255];
const INK: [u8; 3] = [30, 30, 30];

/// Body text of the test page: Swedish UI-like lines, as under a lens.
const BODY: [&str; 6] = [
    "Vill du spara ändringarna innan du stänger?",
    "Dina ändringar går förlorade om du inte sparar dem.",
    "Klockan är 12:34 och dokumentet sparades senast i går.",
    "Öppna inställningarna för att välja språk och tangentbord.",
    "Your changes have been saved to the cloud.",
    "Sök efter filer, mappar och kontakter på datorn.",
];

#[derive(Clone)]
struct Scene {
    bg: [u8; 3],
    fg: [u8; 3],
    /// Font size (em) in physical px.
    em: i32,
    line_height: i32,
    lines: Vec<String>,
    /// Content scrolled up by this many px.
    scroll: i32,
    /// A solid caret bar (x, y, w, h).
    caret: Option<(i32, i32, i32, i32)>,
    /// Frame number of an animated "video" area under the text; `None` = static.
    video: Option<u32>,
}

impl Scene {
    fn page(scale: f64) -> Self {
        let em = (14.0 * scale).round() as i32;
        Scene {
            bg: WHITE,
            fg: INK,
            em,
            line_height: (f64::from(em) * 1.6).round() as i32,
            lines: BODY.iter().map(|s| (*s).to_owned()).collect(),
            scroll: 0,
            caret: None,
            video: None,
        }
    }

    fn margin(&self) -> i32 {
        self.em
    }

    fn baseline(&self, line: usize) -> i32 {
        self.margin() + self.em + line as i32 * self.line_height - self.scroll
    }
}

thread_local! {
    static SCENE: RefCell<Option<Scene>> = const { RefCell::new(None) };
}

fn colorref([r, g, b]: [u8; 3]) -> COLORREF {
    COLORREF(u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16)
}

fn fill(hdc: windows::Win32::Graphics::Gdi::HDC, rect: RECT, rgb: [u8; 3]) {
    unsafe {
        let brush = CreateSolidBrush(colorref(rgb));
        FillRect(hdc, &rect, brush);
        let _ = DeleteObject(brush.into());
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => unsafe {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            SCENE.with_borrow(|scene| {
                let Some(s) = scene else { return };
                fill(hdc, ps.rcPaint, s.bg);
                if let Some(n) = s.video {
                    // Moving gradient stripes and a moving block, changing every frame.
                    let r = ps.rcPaint;
                    let mut x = r.left;
                    while x < r.right {
                        let v = ((x + n as i32 * 7) % 256) as u8;
                        let stripe = RECT {
                            left: x,
                            top: r.top,
                            right: x + 4,
                            bottom: r.bottom,
                        };
                        fill(hdc, stripe, [v, v / 2 + 64, 255 - v]);
                        x += 4;
                    }
                    let bx = (n as i32 * 13) % (r.right - 80).max(1);
                    let block = RECT {
                        left: bx,
                        top: 40,
                        right: bx + 80,
                        bottom: 120,
                    };
                    fill(hdc, block, [250, 220, 40]);
                }
                SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, colorref(s.fg));
                SetTextAlign(hdc, TA_BASELINE | TA_LEFT);
                // A negative height asks for the em size.
                let font = CreateFontW(
                    -s.em,
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
                for (i, line) in s.lines.iter().enumerate() {
                    let text: Vec<u16> = line.encode_utf16().collect();
                    let _ = TextOutW(hdc, s.margin(), s.baseline(i), &text);
                }
                SelectObject(hdc, old);
                let _ = DeleteObject(font.into());
                if let Some((x, y, w, h)) = s.caret {
                    let caret = RECT {
                        left: x,
                        top: y,
                        right: x + w,
                        bottom: y + h,
                    };
                    fill(hdc, caret, s.fg);
                }
            });
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        },
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// A topmost popup at a physical rect showing the thread's `SCENE`.
struct SceneWindow {
    hwnd: HWND,
    rect: PhysRect,
}

impl SceneWindow {
    fn open(rect: PhysRect, scene: Scene) -> Self {
        SCENE.set(Some(scene));
        unsafe {
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(None).unwrap().into();
            let class = w!("PgPipelineChangeWindow");
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
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            pump(Duration::from_millis(300));
            SceneWindow { hwnd, rect }
        }
    }

    /// Change the scene and repaint now.
    fn update(&self, f: impl FnOnce(&mut Scene)) {
        SCENE.with_borrow_mut(|s| f(s.as_mut().expect("scene")));
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
            let _ = UpdateWindow(self.hwnd);
        }
    }
}

impl Drop for SceneWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
        SCENE.set(None);
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
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A capture of the window once its new content has been composed (WGC delivers frames only
/// when the screen changes, at most every 100 ms on Windows 11 24H2).
fn settled(cap: &mut WgcCapturer, win: &SceneWindow) -> Frame {
    pump(Duration::from_millis(250));
    cap.capture(win.rect).unwrap()
}

fn primary() -> PhysRect {
    monitors()
        .unwrap()
        .into_iter()
        .find(|m| m.rect.x == 0 && m.rect.y == 0)
        .expect("a primary monitor at (0, 0)")
        .rect
}

/// Whether the primary monitor fits the lens at `scale` plus margins. GitHub's Windows runners
/// have a 1024×768 screen: enough for 100%, not for the 1200×600 lens of 200%.
fn fits(scale: f64) -> bool {
    let (w, h) = lens_px(LENS, scale);
    let primary = primary();
    primary.width >= w + 100 && primary.height >= h + 100
}

/// Where the test windows go: the primary monitor, which must fit the lens plus margins.
fn origin(scale: f64) -> (i32, i32) {
    assert!(
        fits(scale),
        "the primary monitor is too small for a {}% lens",
        scale * 100.0
    );
    let primary = primary();
    (primary.x + 40, primary.y + 40)
}

fn lens_px((w, h): (u32, u32), scale: f64) -> (u32, u32) {
    (
        (f64::from(w) * scale).round() as u32,
        (f64::from(h) * scale).round() as u32,
    )
}

/// How two captures differ at one grid alignment.
#[derive(Debug, Clone, Copy, Default)]
struct Diff {
    /// Cells whose mean luma moved by more than the default tolerance.
    cells: u32,
    /// Columns × rows of the box around those cells.
    span: (u32, u32),
    /// Largest per-cell mean-luma change.
    max_delta: u8,
    /// Cells over the session 1–9 tolerance (6 levels), for the old rule.
    cells_old: u32,
}

/// Mean luma per 8×8 cell, as `FrameSignature` computes it.
fn cell_lumas(frame: &Frame) -> (u32, Vec<u8>) {
    let cols = frame.width().div_ceil(8);
    let rows = frame.height().div_ceil(8);
    let mut sums = vec![(0u32, 0u32); (cols * rows) as usize];
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            let c = &mut sums[((y / 8) * cols + x / 8) as usize];
            c.0 += u32::from(frame.luma(x, y));
            c.1 += 1;
        }
    }
    (cols, sums.iter().map(|(s, n)| (s / n) as u8).collect())
}

fn diff(a: &Frame, b: &Frame, tolerance: u8) -> Diff {
    let (cols, la) = cell_lumas(a);
    let (_, lb) = cell_lumas(b);
    let mut d = Diff::default();
    let (mut c0, mut c1, mut r0, mut r1) = (u32::MAX, 0, u32::MAX, 0);
    for (i, (x, y)) in la.iter().zip(&lb).enumerate() {
        let delta = x.abs_diff(*y);
        d.max_delta = d.max_delta.max(delta);
        if delta > 6 {
            d.cells_old += 1;
        }
        if delta > tolerance {
            let (c, r) = (i as u32 % cols, i as u32 / cols);
            d.cells += 1;
            (c0, c1, r0, r1) = (c0.min(c), c1.max(c), r0.min(r), r1.max(r));
        }
    }
    if d.cells > 0 {
        d.span = (c1 - c0 + 1, r1 - r0 + 1);
    }
    d
}

/// The session 1–9 rule: more than 0.15% of cells over 6 levels, or any cell in a lens of
/// fewer than 100 cells.
fn old_rule(cells: u32, (w, h): (u32, u32)) -> bool {
    let total = w.div_ceil(8) * h.div_ceil(8);
    cells as f32 / total as f32 > 0.0015 || (cells > 0 && total < 100)
}

/// One measured case over the 64 alignments of the cell grid.
#[derive(Debug)]
struct Case {
    name: String,
    /// Alignments the default `ChangeConfig` calls changed, of `alignments`.
    detected: u32,
    alignments: u32,
    /// The alignment with the fewest and the most changed cells (with `detail`).
    min: Diff,
    max: Diff,
    /// Lowest "largest cell change" over the alignments: tolerances below it see every one.
    min_max_delta: u8,
    /// Alignments the old rule calls changed, at lens 480×200 and 600×300.
    old: (u32, u32),
}

/// Compare states `a` and `b` (captured from a window `LENS` × scale + 8 px) at grid alignments,
/// by cropping. `detail` uses all 64 and adds the per-cell numbers for the table; otherwise 9
/// (the per-pixel rule doesn't depend on the alignment, only the cell means do).
fn case(name: &str, a: &Frame, b: &Frame, scale: f64, detail: bool) -> Case {
    let cfg = ChangeConfig::default();
    let (w, h) = lens_px(LENS, scale);
    // Tagged with the simulated scale, as a capture on such a monitor would be.
    let (a, b) = (
        a.clone().with_origin(a.origin, scale),
        b.clone().with_origin(b.origin, scale),
    );
    let mut c = Case {
        name: format!("{name} @{}%", scale * 100.0),
        detected: 0,
        alignments: 0,
        min: Diff::default(),
        max: Diff::default(),
        min_max_delta: u8::MAX,
        old: (0, 0),
    };
    let mut diffs = Vec::new();
    let offsets: &[u32] = if detail {
        &[0, 1, 2, 3, 4, 5, 6, 7]
    } else {
        &[0, 3, 5]
    };
    for &dy in offsets {
        for &dx in offsets {
            c.alignments += 1;
            let (x, y) = (a.crop(dx, dy, w, h), b.crop(dx, dy, w, h));
            if FrameSignature::of(&y).differs(&FrameSignature::of(&x), &cfg) {
                c.detected += 1;
            }
            if detail {
                diffs.push(diff(&x, &y, cfg.cell_tolerance));
            }
        }
    }
    if detail {
        c.min = *diffs.iter().min_by_key(|d| d.cells).unwrap();
        c.max = *diffs.iter().max_by_key(|d| d.cells).unwrap();
        c.min_max_delta = diffs.iter().map(|d| d.max_delta).min().unwrap();
        // The changed cells sit inside the captured area, so other lens sizes only change the
        // old rule's denominator.
        let count = |size| {
            let px = lens_px(size, scale);
            diffs.iter().filter(|d| old_rule(d.cells_old, px)).count() as u32
        };
        c.old = (count((480, 200)), count((600, 300)));
    }
    c
}

fn print(cases: &[Case]) {
    eprintln!(
        "{:<28} {:>9} {:>9} {:>8} {:>6} {:>9} {:>9} {:>9}",
        "case", "cells min", "cells max", "span max", "Δ min", "detected", "old 480", "old 600"
    );
    for c in cases {
        eprintln!(
            "{:<28} {:>9} {:>9} {:>4}x{:<3} {:>6} {:>6}/{} {:>6}/64 {:>6}/64",
            c.name,
            c.min.cells,
            c.max.cells,
            c.max.span.0,
            c.max.span.1,
            c.min_max_delta,
            c.detected,
            c.alignments,
            c.old.0,
            c.old.1
        );
    }
}

/// Measure every case at one simulated scale.
fn measure(scale: f64, detail: bool) -> Vec<Case> {
    let (w, h) = lens_px(LENS, scale);
    let (x, y) = origin(scale);
    let rect = PhysRect::new(x, y, w + 8, h + 8);
    let mut cap = WgcCapturer::new().unwrap();
    let base = Scene::page(scale);
    let win = SceneWindow::open(rect, base.clone());
    let mut cases = Vec::new();
    let a = settled(&mut cap, &win);

    // Noise: the same static content captured repeatedly at 4 Hz.
    let mut worst = (a.clone(), 0);
    for _ in 0..12 {
        pump(Duration::from_millis(250));
        let f = cap.capture(win.rect).unwrap();
        let n = diff(&a, &f, 0).cells;
        if n >= worst.1 {
            worst = (f, n);
        }
    }
    cases.push(case("static (12 captures)", &a, &worst.0, scale, detail));

    // Caret: a bar after the end of line 0, the width Windows and browsers draw at this scale.
    let caret_w = scale.round().max(1.0) as i32;
    let caret = |s: &Scene| {
        (
            s.margin() + s.em * 22,
            s.baseline(0) - s.em,
            caret_w,
            (f64::from(s.em) * 1.35).round() as i32,
        )
    };
    win.update(|s| s.caret = Some(caret(s)));
    let on = settled(&mut cap, &win);
    win.update(|s| s.caret = None);
    let off = settled(&mut cap, &win);
    cases.push(case("caret blink", &off, &on, scale, detail));
    // The caret inside a word (between two letters of line 1).
    win.update(|s| s.caret = Some((s.margin() + s.em * 3, s.baseline(1) - s.em, caret_w, s.em)));
    let inside = settled(&mut cap, &win);
    win.update(|s| s.caret = None);
    cases.push(case("caret in a word", &off, &inside, scale, detail));

    // One character: the clock's last digit.
    win.update(|s| s.lines[2] = s.lines[2].replace("12:34", "12:35"));
    let digit = settled(&mut cap, &win);
    cases.push(case("one digit changed", &off, &digit, scale, detail));
    win.update(|s| *s = base.clone());
    let a = settled(&mut cap, &win);

    // Typing: one letter appended to a line, then a word.
    win.update(|s| s.lines[3].push('k'));
    let typed = settled(&mut cap, &win);
    cases.push(case("one letter typed", &a, &typed, scale, detail));
    win.update(|s| s.lines[0] = s.lines[0].replace("spara", "stänga"));
    let word = settled(&mut cap, &win);
    cases.push(case("one word changed", &typed, &word, scale, detail));
    // A narrow letter looks like a caret (the known limit of the caret rule).
    win.update(|s| *s = base.clone());
    let a = settled(&mut cap, &win);
    win.update(|s| s.lines[4].push('l'));
    let narrow = settled(&mut cap, &win);
    cases.push(case("letter l typed", &a, &narrow, scale, detail));
    win.update(|s| *s = base.clone());
    let a = settled(&mut cap, &win);

    // Scrolling by a few px (smooth scrolling) and by a line.
    for dy in [1, 2, 4, base.line_height] {
        win.update(|s| s.scroll = dy);
        let scrolled = settled(&mut cap, &win);
        cases.push(case(
            &format!("scroll {dy} px"),
            &a,
            &scrolled,
            scale,
            detail,
        ));
    }
    win.update(|s| *s = base.clone());

    // Video: an animation at ~30 fps behind the text, sampled 250 ms apart.
    let mut frames = Vec::new();
    let start = Instant::now();
    let mut n = 0;
    while frames.len() < 5 {
        n += 1;
        win.update(|s| s.video = Some(n));
        pump(Duration::from_millis(33));
        if start.elapsed() >= Duration::from_millis(250) * frames.len() as u32 {
            frames.push(cap.capture(win.rect).unwrap());
        }
    }
    let video = frames
        .windows(2)
        .map(|p| case("video (250 ms apart)", &p[0], &p[1], scale, detail))
        .min_by_key(|c| c.detected)
        .unwrap();
    cases.push(video);
    cases
}

/// CAP-03 on real captures: a caret blink (after the text or inside a word) never counts as a
/// change, while an edited digit, a typed letter or word, any scroll and video always do, at
/// sampled alignments of the cell grid, at 100% and 200% (200% only where the screen fits it).
#[test]
fn cap03_real_captures_ignore_the_caret_and_see_edits() {
    let _screen = SCREEN.lock().unwrap_or_else(|e| e.into_inner());
    for scale in [1.0, 2.0] {
        // 100% must always run; a larger scale needs a monitor that fits its lens.
        if scale > 1.0 && !fits(scale) {
            eprintln!(
                "skipped {}%: the primary monitor is too small for its lens",
                scale * 100.0
            );
            continue;
        }
        for c in measure(scale, false) {
            let want = match c.name.split(" @").next().unwrap() {
                "static (12 captures)" | "caret blink" | "caret in a word" => 0,
                // Not asserted: the known limit of the caret rule (see ADR-0003).
                "letter l typed" => continue,
                _ => c.alignments,
            };
            assert_eq!(
                c.detected, want,
                "{}: detected at {}/{} alignments",
                c.name, c.detected, c.alignments
            );
        }
    }
}

/// The measurement table recorded in ADR-0003 (all cases, 100/150/200%).
#[test]
#[ignore = "measurement; run with --ignored --nocapture"]
fn measure_change_detection() {
    let _screen = SCREEN.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = Vec::new();
    for scale in [1.0, 1.5, 2.0] {
        all.extend(measure(scale, true));
    }
    print(&all);

    // Cost per re-check: a signature of the new frame and the comparison (unchanged case).
    let cfg = ChangeConfig::default();
    for scale in [1.0, 1.5, 2.0] {
        let (w, h) = lens_px(LENS, scale);
        let frame = Frame::solid(w, h, WHITE).with_origin(PhysRect::new(0, 0, w, h), scale);
        let reference = FrameSignature::of(&frame);
        let mut times: Vec<Duration> = (0..50)
            .map(|_| {
                let t = Instant::now();
                let sig = FrameSignature::of(&frame);
                assert!(!sig.differs(&reference, &cfg));
                t.elapsed()
            })
            .collect();
        times.sort();
        eprintln!("signature + compare {w}x{h}: p50 {:?}", times[25]);
    }
}
