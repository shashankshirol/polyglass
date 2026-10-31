//! Windows Graphics Capture (WGC) region capture (CAP-01, CAP-04; design in ADR-0003).
//!
//! - One capture session per monitor the region touches, created on first use. Each session's
//!   `FrameArrived` handler keeps only the newest frame (older ones go straight back to the
//!   pool), so the pool never fills up and a capture always sees the latest screen contents.
//! - `capture` copies only the region from each monitor's newest frame into a small staging
//!   texture on the GPU and reads that back (CAP-01). Parts of the region outside every monitor
//!   are black.
//! - Monitor rectangles are read with a per-monitor-DPI-aware (v2) thread context, so they are
//!   physical virtual-desktop pixels whatever the process's own DPI awareness is (CAP-04).
//! - Windows with `WDA_EXCLUDEFROMCAPTURE` display affinity are left out of WGC frames by the
//!   OS; the lens sets it on itself (CAP-02, `apps/desktop`).
//! - `release` closes all sessions while the lens is hidden (LENS-06).
//!
//! Frames stay in memory (PRIV-02).
//!
//! `unsafe` is allowed in this module only (see `lib.rs`): Direct3D 11, DXGI interop and the
//! monitor APIs are plain Win32/COM calls that windows-rs exposes as `unsafe fn`. Each block
//! states why it is sound.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use pg_core::{Error, Frame, PhysRect, PixelFormat, Result};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureAccess,
    GraphicsCaptureAccessKind, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, LPARAM, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetThreadDpiAwarenessContext,
};
use windows::core::{BOOL, IInspectable, Interface};

use crate::ScreenCapturer;

/// How long `capture` waits for a new session's first frame.
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_millis(500);
/// Sessions for monitors the region left are closed after this long unused.
const UNUSED_SESSION_TIMEOUT: Duration = Duration::from_secs(5);
/// Upper bound on frame delivery (Windows 11 24H2+; ignored where unsupported). The pipeline
/// rechecks at 4 Hz, so faster frames would only cost GPU copies.
const MIN_UPDATE_INTERVAL: Duration = Duration::from_millis(100);

/// A monitor in physical virtual-desktop pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorInfo {
    /// `HMONITOR` value; only used as an identity.
    pub id: isize,
    pub rect: PhysRect,
    /// 1.0 = 100%.
    pub scale_factor: f64,
}

/// All monitors, in physical pixels (queried now, so hot-plug and scale changes are seen).
pub fn monitors() -> Result<Vec<MonitorInfo>> {
    unsafe extern "system" fn collect(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `&mut Vec<HMONITOR>` passed to EnumDisplayMonitors below, which
        // outlives the (synchronous) enumeration.
        let out = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
        out.push(h);
        true.into()
    }

    let _dpi = PerMonitorDpiScope::enter();
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: the callback only writes to `handles`, passed by pointer for the call's duration.
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut handles as *mut Vec<HMONITOR> as isize),
        )
    };
    if !ok.as_bool() {
        return Err(Error::Capture("EnumDisplayMonitors failed".into()));
    }
    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `h` comes from the enumeration; `info` is a valid, sized MONITORINFO.
        if !unsafe { GetMonitorInfoW(h, &mut info) }.as_bool() {
            continue; // Unplugged since enumeration.
        }
        let (mut dpi_x, mut dpi_y) = (96, 96);
        // SAFETY: valid monitor handle and out-pointers to locals.
        let scale = match unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }
        {
            Ok(()) => f64::from(dpi_x) / 96.0,
            Err(_) => 1.0,
        };
        let r = info.rcMonitor;
        out.push(MonitorInfo {
            id: h.0 as isize,
            rect: PhysRect::new(
                r.left,
                r.top,
                (r.right - r.left) as u32,
                (r.bottom - r.top) as u32,
            ),
            scale_factor: scale,
        });
    }
    Ok(out)
}

/// Makes the current thread per-monitor DPI aware (v2) until dropped.
struct PerMonitorDpiScope(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT);

impl PerMonitorDpiScope {
    fn enter() -> Self {
        // SAFETY: plain Win32 call on the current thread; the previous context is restored on
        // drop.
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) })
    }
}

impl Drop for PerMonitorDpiScope {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: restores the context returned by `enter` on the same thread.
            unsafe { SetThreadDpiAwarenessContext(self.0) };
        }
    }
}

type FrameSlot = Arc<(Mutex<Option<Direct3D11CaptureFrame>>, Condvar)>;

/// A running capture of one monitor.
struct MonitorSession {
    monitor: MonitorInfo,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    token: i64,
    newest: FrameSlot,
    last_used: Instant,
}

impl MonitorSession {
    fn start(device: &IDirect3DDevice, monitor: MonitorInfo) -> Result<Self> {
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
            .map_err(cap_err)?;
        // SAFETY: `monitor.id` is an HMONITOR from `monitors()`; if it has gone away the call
        // fails with an error rather than misbehaving.
        let item: GraphicsCaptureItem =
            unsafe { interop.CreateForMonitor(HMONITOR(monitor.id as *mut _)) }.map_err(cap_err)?;
        let size = item.Size().map_err(cap_err)?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )
        .map_err(cap_err)?;
        let newest: FrameSlot = Arc::new((Mutex::new(None), Condvar::new()));
        let slot = newest.clone();
        let token = pool
            .FrameArrived(
                &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(
                    move |pool, _| {
                        let Some(pool) = pool.as_ref() else {
                            return Ok(());
                        };
                        let frame = pool.TryGetNextFrame()?;
                        let (lock, cv) = &*slot;
                        let old = lock
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .replace(frame);
                        if let Some(old) = old {
                            // Return the buffer to the pool now rather than when dropped.
                            let _ = old.Close();
                        }
                        cv.notify_all();
                        Ok(())
                    },
                ),
            )
            .map_err(cap_err)?;
        let session = pool.CreateCaptureSession(&item).map_err(cap_err)?;
        // Optional properties: missing on older Windows 10/11 builds.
        let _ = session.SetIsCursorCaptureEnabled(false);
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetMinUpdateInterval(TimeSpan::from(MIN_UPDATE_INTERVAL));
        session.StartCapture().map_err(cap_err)?;
        Ok(Self {
            monitor,
            pool,
            session,
            token,
            newest,
            last_used: Instant::now(),
        })
    }

    /// Run `f` on the newest frame's texture, waiting for a new session's first frame. The frame
    /// stays locked meanwhile, so the arrival handler can't hand its buffer back to the pool
    /// (and WGC can't overwrite it) during the copy.
    fn with_newest_texture<T>(&self, f: impl FnOnce(&ID3D11Texture2D) -> Result<T>) -> Result<T> {
        let (lock, cv) = &*self.newest;
        let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let (guard, _) = cv
            .wait_timeout_while(guard, FIRST_FRAME_TIMEOUT, |f| f.is_none())
            .unwrap_or_else(|e| e.into_inner());
        let frame = guard
            .as_ref()
            .ok_or_else(|| Error::Capture("no frame from Windows Graphics Capture".into()))?;
        let access: IDirect3DDxgiInterfaceAccess =
            frame.Surface().and_then(|s| s.cast()).map_err(cap_err)?;
        // SAFETY: COM call on the live surface of a frame we hold; returns a new reference.
        let texture = unsafe { access.GetInterface::<ID3D11Texture2D>() }.map_err(cap_err)?;
        f(&texture)
    }
}

impl Drop for MonitorSession {
    fn drop(&mut self) {
        let _ = self.pool.RemoveFrameArrived(self.token);
        let _ = self.session.Close();
        if let Some(f) = self
            .newest
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = f.Close();
        }
        let _ = self.pool.Close();
    }
}

/// [`ScreenCapturer`] using Windows Graphics Capture (Windows 10 2004+).
pub struct WgcCapturer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    sessions: HashMap<isize, MonitorSession>,
    /// Reused CPU-readable texture, recreated when the region size changes.
    staging: Option<(u32, u32, ID3D11Texture2D)>,
    self_excluded: bool,
}

impl fmt::Debug for WgcCapturer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WgcCapturer")
            .field("sessions", &self.sessions.len())
            .field("self_excluded", &self.self_excluded)
            .finish_non_exhaustive()
    }
}

impl WgcCapturer {
    /// Create a capturer on the default hardware GPU. Fails if WGC is unsupported.
    pub fn new() -> Result<Self> {
        if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
            return Err(Error::Capture(
                "Windows Graphics Capture is not supported on this system".into(),
            ));
        }
        let mut device = None;
        let mut context = None;
        // SAFETY: standard device creation with out-pointers to locals.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .map_err(cap_err)?;
        let device: ID3D11Device =
            device.ok_or_else(|| Error::Capture("no Direct3D device".into()))?;
        let context = context.ok_or_else(|| Error::Capture("no Direct3D context".into()))?;
        // Frame-arrived callbacks touch the device from WGC's threads while `capture` uses the
        // immediate context: let D3D serialize them.
        if let Ok(mt) = device.cast::<ID3D11Multithread>() {
            // SAFETY: plain COM call on a live interface.
            unsafe {
                let _ = mt.SetMultithreadProtected(true);
            }
        }
        // Windows 11 shows a yellow border around captured monitors unless borderless capture
        // is allowed; unpackaged apps are granted it without a prompt. Failure keeps the border.
        if let Ok(op) =
            GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless)
        {
            let _ = op.join();
        }
        Ok(Self {
            device,
            context,
            sessions: HashMap::new(),
            staging: None,
            self_excluded: false,
        })
    }

    /// Declare that every Polyglass window has `WDA_EXCLUDEFROMCAPTURE` set, so frames can't
    /// contain them (reported by [`ScreenCapturer::excludes_self`], CAP-02).
    pub fn with_self_excluded(mut self, excluded: bool) -> Self {
        self.self_excluded = excluded;
        self
    }

    /// The WinRT view of the D3D device, which frame pools need. Made on demand because the
    /// WinRT interface isn't `Send` (the D3D device itself is).
    fn winrt_device(&self) -> Result<IDirect3DDevice> {
        let dxgi: IDXGIDevice = self.device.cast().map_err(cap_err)?;
        // SAFETY: wraps a live DXGI device for WinRT; returns a new reference.
        let inspectable =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.map_err(cap_err)?;
        inspectable.cast().map_err(cap_err)
    }

    /// Number of running monitor sessions (0 after [`ScreenCapturer::release`]).
    pub fn active_sessions(&self) -> usize {
        self.sessions.len()
    }

    fn staging(&mut self, w: u32, h: u32) -> Result<ID3D11Texture2D> {
        if let Some((sw, sh, tex)) = &self.staging
            && (*sw, *sh) == (w, h)
        {
            return Ok(tex.clone());
        }
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut tex = None;
        // SAFETY: valid descriptor, no initial data, out-pointer to a local.
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut tex)) }.map_err(cap_err)?;
        let tex = tex.ok_or_else(|| Error::Capture("no staging texture".into()))?;
        self.staging = Some((w, h, tex.clone()));
        Ok(tex)
    }

    /// Copy `part` (virtual-desktop pixels, inside `session`'s monitor) into `out` at its offset
    /// from `region`.
    fn copy_part(
        &mut self,
        id: isize,
        part: PhysRect,
        region: PhysRect,
        out: &mut [u8],
    ) -> Result<()> {
        let mon = self.sessions[&id].monitor.rect;
        let x0 = (part.x - mon.x) as u32;
        let y0 = (part.y - mon.y) as u32;
        let staging = self.staging(part.width, part.height)?;
        let context = self.context.clone();
        let session = &self.sessions[&id];
        session.with_newest_texture(|src| {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            // SAFETY: plain COM getter into a local.
            unsafe { src.GetDesc(&mut desc) };
            // The frame can be smaller than the monitor right after a resolution change; the
            // rest of the part stays black.
            let x1 = (x0 + part.width).min(desc.Width);
            let y1 = (y0 + part.height).min(desc.Height);
            if x1 <= x0 || y1 <= y0 {
                return Ok(());
            }
            let (w, h) = (x1 - x0, y1 - y0);
            let src_box = D3D11_BOX {
                left: x0,
                top: y0,
                front: 0,
                right: x1,
                bottom: y1,
                back: 1,
            };
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            // SAFETY: both textures are live and BGRA8; the box lies inside `src` (clamped to its
            // size above) and fits the staging texture (`part`-sized, w <= part.width,
            // h <= part.height).
            unsafe {
                context.CopySubresourceRegion(&staging, 0, 0, 0, 0, src, 0, Some(&src_box));
                context
                    .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                    .map_err(cap_err)?;
            }
            let out_stride = region.width as usize * 4;
            let dx = (part.x - region.x) as usize * 4;
            let dy = (part.y - region.y) as usize;
            let row_bytes = w as usize * 4;
            for row in 0..h as usize {
                // SAFETY: the mapping holds at least `h` rows of `RowPitch` bytes, each at least
                // `w * 4` bytes long, until Unmap below.
                let src_row = unsafe {
                    std::slice::from_raw_parts(
                        (mapped.pData as *const u8).add(row * mapped.RowPitch as usize),
                        row_bytes,
                    )
                };
                let start = (dy + row) * out_stride + dx;
                out[start..start + row_bytes].copy_from_slice(src_row);
            }
            // SAFETY: matches the successful Map above.
            unsafe { context.Unmap(&staging, 0) };
            Ok(())
        })
    }
}

impl ScreenCapturer for WgcCapturer {
    fn capture(&mut self, region: PhysRect) -> Result<Frame> {
        if region.is_empty() {
            return Err(Error::InvalidInput("empty capture region".into()));
        }
        let parts: Vec<(MonitorInfo, PhysRect)> = monitors()?
            .into_iter()
            .filter_map(|m| m.rect.intersect(&region).map(|p| (m, p)))
            .collect();
        if parts.is_empty() {
            return Err(Error::Capture("region is outside every monitor".into()));
        }
        let now = Instant::now();
        for (m, _) in &parts {
            // A monitor whose geometry or scale changed gets a fresh session.
            if self.sessions.get(&m.id).is_some_and(|s| s.monitor != *m) {
                self.sessions.remove(&m.id);
            }
            if !self.sessions.contains_key(&m.id) {
                let session = MonitorSession::start(&self.winrt_device()?, *m)?;
                self.sessions.insert(m.id, session);
            }
            if let Some(s) = self.sessions.get_mut(&m.id) {
                s.last_used = now;
            }
        }
        self.sessions
            .retain(|_, s| now.duration_since(s.last_used) < UNUSED_SESSION_TIMEOUT);

        // Black, opaque background for parts outside every monitor.
        let mut data = vec![0u8; region.width as usize * region.height as usize * 4];
        for px in data.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        for (m, part) in &parts {
            self.copy_part(m.id, *part, region, &mut data)?;
        }
        // Copied pixels carry the desktop's alpha; make the frame opaque throughout.
        for px in data.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        // The scale of the monitor holding most of the region.
        let scale = parts
            .iter()
            .max_by_key(|(_, p)| u64::from(p.width) * u64::from(p.height))
            .map_or(1.0, |(m, _)| m.scale_factor);
        Ok(Frame::new(
            region.width,
            region.height,
            region.width as usize * 4,
            PixelFormat::Bgra8,
            data,
        )?
        .with_origin(region, scale))
    }

    fn excludes_self(&self) -> bool {
        self.self_excluded
    }

    fn release(&mut self) {
        self.sessions.clear();
        self.staging = None;
    }
}

/// WinRT/COM errors carry an HRESULT and a system message only.
fn cap_err(e: windows::core::Error) -> Error {
    Error::Capture(format!("Windows Graphics Capture: {e}"))
}
