# ADR-0003: Windows region capture and self-exclusion

- **Status:** accepted for Windows (macOS ScreenCaptureKit and X11 parts of M0-SPK-2 still open)
- **Date:** 2026-10-31
- **Requirements:** CAP-01, CAP-02, CAP-03, CAP-04, LENS-06, PRIV-02, PERF-03

## Context

The pipeline captures the lens interior at rest and re-checks it at 4 Hz (CAP-03). Frames must
never contain the lens itself, or OCR would read the translations the lens draws (CAP-02).
Regions are physical virtual-desktop pixels and can sit on, or straddle, monitors with
different scale factors (CAP-04). The dev machine has 2560×1440 at 100% (primary) and
2160×3840 at 150% to its left, at x −2160..0, y −2398..1442; the two share the edge x = 0.

## Options

| Option | Pros | Cons | Measurements |
|---|---|---|---|
| A. Windows Graphics Capture (WGC), monitor item, crop on the GPU | Respects `WDA_EXCLUDEFROMCAPTURE`; GPU copy of just the region; works per monitor at native resolution; no cursor if asked | D3D11/DXGI interop is `unsafe` in windows-rs; frames arrive only when the screen changes, so the newest one must be kept; a session per monitor | 600×300: p50 0.26 ms, p95 0.41 ms (release); first capture after a session starts 10–37 ms |
| B. WGC per capture (start a session, take one frame, close) | No long-lived state | Session start on every 4 Hz re-check | First-frame cost above (10–37 ms) per capture |
| C. DXGI Desktop Duplication | Fast, dirty rects | One duplication per output and adapter; whole-output frames and access-lost handling; behaviour with `WDA_EXCLUDEFROMCAPTURE` not checked here; more code than A | Not tried |
| D. GDI `BitBlt` from the screen DC | Simple | Slow on large DCs, ignores DirectX/protected content, blocks on DWM | Not tried |

## Decision

**Option A.** `pg_capture::WgcCapturer` (`crates/pg-capture/src/wgc.rs`):

- `capture(region)` enumerates monitors on every call (`EnumDisplayMonitors` +
  `GetMonitorInfoW` + `GetDpiForMonitor`) inside a per-monitor-DPI-aware-v2 thread scope, so
  rectangles are physical pixels whatever the process's own DPI awareness is (the Tauri app is
  PMv2; test binaries are not).
- One `GraphicsCaptureSession` per monitor the region touches, created on first use and
  re-created if that monitor's rect or scale changes. Sessions for monitors the region left
  close after 5 s unused. Cursor capture is off; borderless capture is requested (Windows 11)
  and `MinUpdateInterval` is 100 ms where supported (Windows 11 24H2+).
- Each session's free-threaded frame pool (2 buffers) has a `FrameArrived` handler that keeps
  only the newest frame and closes the previous one. Without it the pool fills between 4 Hz
  re-checks, WGC drops newer frames, and a capture could return content that is up to one
  re-check old and stays old once the screen stops changing.
- The copy holds the newest frame's lock, copies just the monitor-local part of the region into
  a reusable staging texture (`CopySubresourceRegion`), maps it and writes rows into a BGRA
  `Frame`. Parts of the region outside every monitor are black; alpha is forced to 255.
  `Frame.scale_factor` is the scale of the monitor holding most of the region.
- `ScreenCapturer::release` (new, default no-op) drops all sessions. The pipeline driver calls
  it on the blocking pool when the lens is hidden or paused (LENS-06), so a hidden lens holds no
  capture session and gets no frame callbacks.
- The D3D device is multithread-protected (`ID3D11Multithread`) because frame callbacks arrive on
  WGC's threads. The WinRT `IDirect3DDevice` wrapper is not `Send` in windows-rs, so it is made
  on demand when a session starts instead of being stored.
- `unsafe` is allowed for the `wgc` module only, with a `SAFETY` comment on each block.

**Self-exclusion (CAP-02):** `apps/desktop` calls `SetWindowDisplayAffinity(hwnd,
WDA_EXCLUDEFROMCAPTURE)` on the lens before it is first shown. WGC (and screenshots) then leave
the lens out. If the call fails (Windows older than 10 2004), live mode does not start and the
lens says why (`cap-error-self-exclusion`) instead of reading its own output.

**Default pipeline:** without `--demo`, the app runs `WgcCapturer` + `WindowsOcr` +
`MockTranslator` with source Auto (sv/en) → en. Until the on-device translator (M1-TR-1),
Swedish blocks show as `[en] <recognized text>`, English passes through, and the toolbar shows a
"MOCK MT" badge.

## Verified on this machine (Windows 11, debug unless noted)

Automated, `crates/pg-capture/tests/wgc.rs` (opens its own topmost Win32 windows):

- `cap01_captures_exactly_the_region`: exact pixels inside a window and an exact edge at the
  window border.
- `cap04_every_monitor_and_a_region_across_two`: a window on each monitor (100% and 150%,
  negative coordinates) is captured exactly with the right `scale_factor`; a 200×40 region
  across x = 0 gets the left window's colour in its left half and the right window's in its right
  half.
- `cap02_excluded_window_is_not_captured`: the same window is captured before and not after
  `WDA_EXCLUDEFROMCAPTURE`.
- `cap02_marker_is_never_recognized_over_100_cycles`: a window drawing "POLYGLASS MARKER QX7"
  with `WDA_EXCLUDEFROMCAPTURE`; Windows OCR over 100 capture cycles never reads "MARKER". A
  control run without exclusion does read it, so the test can detect a leak. This is the SPEC
  CAP-02 test with a Win32 window standing in for the lens; the lens uses the same call.
- `release_closes_sessions_and_capture_reopens_them`.
- `measure_capture_latency` (ignored; release): p50 0.26 ms / p95 0.41 ms for 600×300 on both
  monitors; first capture 36.9 ms (primary) and 9.7 ms (150% monitor).

Manual, `npx tauri dev` (live mode), reading the lens's text through UI Automation because the
lens is invisible to screenshots:

- A window with "Vill du spara ändringarna innan du stänger?" and "Your changes have been
  saved." under the lens on the 100% monitor: the lens showed `[en] Vill du spara ändringarna
  innan du stänger?` and the English line unchanged. Same result after moving the lens to the
  150% monitor (720×300 physical at 144 DPI).
- CPU: 438 ms per 10 s with the lens visible over static content (4 Hz re-check: capture +
  change detection, plus the click-through poller; debug build), 0 ms per 10 s when hidden.
- Logs at the default level contained no recognized text (PRIV-07).

## Not yet verified

- Whether Windows 11 draws the yellow capture border around monitors. Borderless access is
  requested, but the border can't be seen in screenshots; check by eye.
- A real lens drawing the marker over fixture text (the automated test uses a stand-in window).
- Release-build CPU of the live app; GPU cost of keeping sessions open while visible.
- Monitor hot-plug, resolution change during a session, HDR monitors, and GPUs switching
  (device-lost). A failed capture surfaces as `cap-error` in the lens; recovery isn't tested.
- The lens straddling two monitors with different DPI: the region is computed from the lens's own
  scale factor (ADR-0002); the capture side is tested, the window side is not.
- CI: GitHub's Windows runners may not support WGC (no interactive desktop); the tests would
  fail there rather than skip. Decide when CI first runs (M0-REPO-3).

## Consequences

- Capture cost is negligible next to OCR (≈ 19 ms p50 at 600×300); PERF-03 is met with
  margin on this machine.
- Change detection (`ChangeConfig`) can now be tuned on real captures.
- macOS (ScreenCaptureKit) implements the same trait with `release` for LENS-06.
