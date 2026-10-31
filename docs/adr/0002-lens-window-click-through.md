# ADR-0002: Lens window and click-through interior

- **Status:** accepted for Windows; proposed for macOS and Linux (not yet measured there)
- **Date:** 2026-10-31
- **Requirements:** LENS-01, LENS-02, LENS-03, LENS-04, LENS-06, SEC-01, SEC-02, CAP-04

## Context

The lens is a frameless, transparent, always-on-top Tauri 2 window. Users must be able to click
and scroll the content under the lens interior, while the toolbar and frame stay interactive (to
drag, resize and use controls). Tauri's `set_ignore_cursor_events` applies to the whole window,
and no portable per-region hit-test API exists across Windows, macOS and Linux (PLAN §5).

## Options

| Option | Pros | Cons | Measurements (Windows) |
|---|---|---|---|
| A. Poll the cursor at ~30 Hz and toggle `set_ignore_cursor_events` when it enters or leaves the interior | Portable: uses only Tauri APIs. Simple. Costs nothing while hidden. | Up to one poll interval of lag. A background thread. | 19–49 ms to switch, about 0.3% of one core while visible |
| B. Native per-region hit-testing (`WM_NCHITTEST` → `HTTRANSPARENT`, `NSView hitTest:`, X11 input shapes) | No lag, no polling | Three native implementations plus `unsafe`. On Windows `HTTRANSPARENT` only passes clicks to windows of the same thread, not to other apps. | Not tried |
| C. Separate windows for chrome and interior | Each window has its own ignore flag | The windows drift apart while dragging. Twice the z-order and focus problems. | Not tried |
| D. Hold a modifier to interact with content below (PLAN §10 fallback) | Trivial | Worse UX; fails LENS-04 | n/a |

## Decision

**Option A.** `apps/desktop/src-tauri/src/clickthrough.rs` runs a thread named
`pg-clickthrough`:

- Each tick it reads the global cursor position (`AppHandle::cursor_position`, physical px). It
  sets "ignore" only when the cursor is inside the interior rectangle, and calls
  `set_ignore_cursor_events` only when that state changes.
- The interior rectangle is cached from window `Moved`/`Resized`/`ScaleFactorChanged` events
  (`Layout::interior` → `LogicalRect::to_physical`), so the poller never queries the window. The
  same rectangle is the capture region sent to the pipeline driver.
- The interior insets (frame 6 px, toolbar 30 px, logical) are defined once in Rust (`LAYOUT`).
  The UI reads them via `lens_config`, so the CSS and the hit-test can't drift apart.
- While the lens is hidden or click-through is switched off, the thread is parked
  (`thread::park`): no polling at all (LENS-06).

### Verified on Windows 11 (this dev machine)

Tauri 2.11.6 and WebView2 156. Monitors: 2560×1440 at 100% (primary) and 2160×3840 at 150%
(above-left, negative coordinates). Checked with a Win32 probe script (`SetCursorPos`,
`GetWindowLongPtr(GWL_EXSTYLE)`, `WindowFromPoint`, synthetic mouse and keyboard input) on a debug
build, in both `tauri dev` and embedded-asset (`custom-protocol`) modes:

- Cursor over the interior: `WS_EX_TRANSPARENT` is set, and `WindowFromPoint` returns the window
  underneath (another app, or the desktop on the 150% monitor). Over the toolbar, the left frame
  or the bottom frame, the lens stays the hit window.
- Switch latency from cursor move to style change: 19–49 ms across 7 samples, which matches the
  33 ms poll interval plus a main-thread dispatch.
- CPU (debug build, 10 s windows): 31 ms with the lens visible and the pipeline paused (poller
  only, ≈0.3% of a core); 0 ms with the lens hidden, so the poller and the pipeline are both parked;
  125 ms with the demo pipeline also re-checking at 4 Hz.
- Placement and window behaviour:
  - First launch is centred on the cursor's monitor work area at 480×200 (LENS-01) and stays
    on top.
  - The lens has no taskbar button (checked on a taskbar screenshot).
  - Dragging by the toolbar works, the arrow keys move it 1 px and Shift+arrow 10 px (LENS-02).
  - Resizing from a corner and an edge works, and the window stops at 120×48 (LENS-03).
- Dragging onto the 150% monitor resizes the window to 720×300 physical (480×200 logical). The
  pipeline gets the new physical region and redraws (CAP-04 path).

### Not yet verified

- **macOS:** `set_ignore_cursor_events` maps to `NSWindow.ignoresMouseEvents`. The transparent
  window needs `macOSPrivateApi` (enabled). Polling `cursor_position` from a background thread
  must be confirmed not to need the main thread on AppKit. Measure latency and CPU there.
- **Linux X11:** tao implements ignore-cursor-events with an empty input shape, and transparency
  needs a compositor. Wayland can't place the window globally or read the global cursor, so
  Freeze mode applies there (PLAN §5, M2-3).
- Release-build numbers. Debug numbers are an upper bound.
- The cursor is not checked against mouse-button state. If a button is held while the cursor
  crosses into the interior (for example a text selection that started outside), the window still
  switches to click-through. No problem seen while dragging or resizing, because the OS move/resize
  loop keeps the cursor on the chrome.

## Consequences

- One ≤ 50 ms lag when the cursor moves from the chrome into the interior. It hasn't been
  judged by a person using it yet (only measured). A native Windows hook (option B) is possible later behind the same
  `ClickThrough` API if users report it.
- Other chrome (Panel mode strip, tooltips) must be reflected in `Layout::interior`, or it
  will be click-through.
- With click-through off (toolbar toggle), the interior gets a near-invisible 1% fill so the
  webview receives clicks there.
- Follow-ups recorded in CHECKLIST.md: macOS/Linux measurements (M0-SPK-1), the click-through
  hotkey (LENS-04 / M1-APP-9), and restoring the last position (LENS-01 / M1-APP-2).
