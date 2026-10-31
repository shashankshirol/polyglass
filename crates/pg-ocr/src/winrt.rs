//! Windows OCR engine backed by WinRT `Windows.Media.Ocr` (OCR-01, OCR-06, SV-02).
//!
//! - Languages are whatever Windows reports in `OcrEngine.AvailableRecognizerLanguages` at the
//!   time of the call (OCR-06). Swedish needs the optional "Swedish OCR" feature; without it,
//!   asking for Swedish returns [`Error::OcrLanguageMissing`] so the UI can explain how to add it
//!   (SV-02).
//! - Windows OCR reports word boxes; a line's box is the union of its words' boxes, in frame
//!   pixels.
//! - Windows OCR has no confidence score, so every line reports 1.0 (OCR-05 filtering is a no-op
//!   with this engine).
//! - Windows OCR misses or misreads words in UI text of ~16 px and smaller. Frames below 200%
//!   scale are enlarged 2× (bilinear) before recognition, which costs a few ms and fixes most of
//!   that (M1-OCR-5; measurements in CHECKLIST). 3× was no better.
//! - The frame is copied into a `SoftwareBitmap` through a WinRT buffer, so no `unsafe` code is
//!   needed. Pixels stay in memory (PRIV-02).

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use pg_core::{Error, Frame, LangTag, Line, PixelFormat, Rect, Result};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine as WinOcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::core::HSTRING;

use crate::OcrEngine;

/// [`OcrEngine`] using the OCR built into Windows 10/11.
pub struct WindowsOcr {
    /// Recognizers already created, by the Windows language tag they were created for.
    engines: Mutex<HashMap<LangTag, WinOcrEngine>>,
    max_dimension: u32,
    /// Fixed enlargement factor; `None` picks one per frame (see [`WindowsOcr::upscale_for`]).
    upscale: Option<u32>,
}

impl WindowsOcr {
    /// Create the engine. Fails only if the WinRT OCR API itself is unavailable.
    pub fn new() -> Result<Self> {
        let max_dimension = WinOcrEngine::MaxImageDimension().map_err(win_err)?;
        Ok(Self {
            engines: Mutex::new(HashMap::new()),
            max_dimension,
            upscale: None,
        })
    }

    /// Always enlarge frames by `factor` (1–4) before recognition instead of choosing per
    /// frame. Boxes are still reported in frame pixels.
    pub fn with_upscale(mut self, factor: u32) -> Self {
        self.upscale = Some(factor.clamp(1, 4));
        self
    }

    /// Enlargement for `frame`: 2× below 200% scale (where UI text is small in physical
    /// pixels), else 1×, reduced if the result would exceed the engine's size limit.
    fn upscale_for(&self, frame: &Frame) -> u32 {
        let wanted = self
            .upscale
            .unwrap_or(if frame.scale_factor < 2.0 { 2 } else { 1 });
        let largest = frame.width().max(frame.height()).max(1);
        wanted.min(self.max_dimension / largest).max(1)
    }

    /// Languages installed for Windows OCR, queried now (OCR-06).
    pub fn available_languages() -> Result<Vec<LangTag>> {
        let langs = WinOcrEngine::AvailableRecognizerLanguages().map_err(win_err)?;
        let mut out = Vec::new();
        for lang in &langs {
            let tag = lang.LanguageTag().map_err(win_err)?;
            // Tags Windows reports but we can't represent (none known) are skipped.
            if let Ok(tag) = LangTag::parse(&tag.to_string_lossy()) {
                out.push(tag);
            }
        }
        Ok(out)
    }

    /// The recognizer for `lang`, created on first use.
    fn engine_for(&self, lang: Option<&LangTag>) -> Result<WinOcrEngine> {
        let tag = match lang {
            Some(wanted) => crate::resolve_language(&Self::available_languages()?, wanted)?,
            None => {
                // No preference: the user's profile languages, else the first installed one.
                if let Ok(engine) = WinOcrEngine::TryCreateFromUserProfileLanguages() {
                    return Ok(engine);
                }
                Self::available_languages()?
                    .into_iter()
                    .next()
                    .ok_or_else(|| Error::Ocr("no Windows OCR languages are installed".into()))?
            }
        };
        let mut engines = self.engines.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(engine) = engines.get(&tag) {
            return Ok(engine.clone());
        }
        let language = Language::CreateLanguage(&HSTRING::from(tag.as_str())).map_err(win_err)?;
        // Returns an error (null engine) if the language was removed since we listed it.
        let engine = WinOcrEngine::TryCreateFromLanguage(&language)
            .map_err(|_| Error::OcrLanguageMissing(tag.clone()))?;
        engines.insert(tag, engine.clone());
        Ok(engine)
    }

    fn to_bitmap(&self, frame: &Frame, k: u32) -> Result<SoftwareBitmap> {
        let (w, h) = (frame.width() * k, frame.height() * k);
        if w == 0 || h == 0 {
            return Err(Error::InvalidInput("empty frame".into()));
        }
        if w > self.max_dimension || h > self.max_dimension {
            return Err(Error::InvalidInput(format!(
                "frame {w}x{h} exceeds the Windows OCR limit of {}",
                self.max_dimension
            )));
        }
        let bgra = upscale_bilinear(&to_bgra(frame), frame.width(), frame.height(), k);
        let writer = DataWriter::new().map_err(win_err)?;
        writer.WriteBytes(&bgra).map_err(win_err)?;
        let buffer = writer.DetachBuffer().map_err(win_err)?;
        SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, w as i32, h as i32)
            .map_err(win_err)
    }
}

impl fmt::Debug for WindowsOcr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowsOcr")
            .field("max_dimension", &self.max_dimension)
            .field("upscale", &self.upscale)
            .finish_non_exhaustive()
    }
}

impl OcrEngine for WindowsOcr {
    fn id(&self) -> &'static str {
        "windows-ocr"
    }

    fn recognize(&self, frame: &Frame, lang: Option<&LangTag>) -> Result<Vec<Line>> {
        let engine = self.engine_for(lang)?;
        let k = self.upscale_for(frame);
        let bitmap = self.to_bitmap(frame, k)?;
        let result = engine
            .RecognizeAsync(&bitmap)
            .and_then(|op| op.join())
            .map_err(win_err)?;
        let mut lines = Vec::new();
        for line in &result.Lines().map_err(win_err)? {
            let mut bbox: Option<Rect> = None;
            for word in &line.Words().map_err(win_err)? {
                let r = word.BoundingRect().map_err(win_err)?;
                let r = Rect::new(r.X, r.Y, r.Width, r.Height).scaled(1.0 / k as f32);
                bbox = Some(bbox.map_or(r, |b| b.union(&r)));
            }
            let text = line.Text().map_err(win_err)?.to_string_lossy();
            if let Some(bbox) = bbox
                && !text.trim().is_empty()
            {
                lines.push(Line::new(text, bbox, 1.0));
            }
        }
        Ok(lines)
    }

    fn supported_languages(&self) -> Vec<LangTag> {
        Self::available_languages().unwrap_or_default()
    }
}

/// Tightly packed, opaque BGRA copy of `frame`.
fn to_bgra(frame: &Frame) -> Vec<u8> {
    let (w, h) = (frame.width() as usize, frame.height() as usize);
    let bpp = frame.format().bytes_per_pixel();
    let mut out = Vec::with_capacity(w * h * 4);
    for row in frame.data().chunks(frame.stride()).take(h) {
        for p in row[..w * bpp].chunks_exact(bpp) {
            match frame.format() {
                PixelFormat::Bgra8 => out.extend_from_slice(&[p[0], p[1], p[2], 255]),
                PixelFormat::Rgba8 => out.extend_from_slice(&[p[2], p[1], p[0], 255]),
                PixelFormat::Gray8 => out.extend_from_slice(&[p[0], p[0], p[0], 255]),
            }
        }
    }
    out
}

/// Enlarge a packed BGRA image by an integer factor with bilinear filtering (pixel centres
/// aligned, edges clamped).
fn upscale_bilinear(src: &[u8], w: u32, h: u32, k: u32) -> Vec<u8> {
    if k == 1 {
        return src.to_vec();
    }
    let (w, h, k) = (w as usize, h as usize, k as usize);
    let (ow, oh) = (w * k, h * k);
    let mut out = vec![255u8; ow * oh * 4];
    // Source coordinate of each output column/row: (o + 0.5) / k - 0.5, split into index + weight.
    let map = |o: usize, n: usize| -> (usize, usize, u32) {
        let s = ((2 * o + 1) as f32 / (2 * k) as f32 - 0.5).max(0.0);
        let i0 = (s as usize).min(n - 1);
        let i1 = (i0 + 1).min(n - 1);
        (i0, i1, ((s - i0 as f32) * 256.0) as u32)
    };
    let cols: Vec<_> = (0..ow).map(|x| map(x, w)).collect();
    for oy in 0..oh {
        let (y0, y1, fy) = map(oy, h);
        let (r0, r1) = (&src[y0 * w * 4..], &src[y1 * w * 4..]);
        let dst = &mut out[oy * ow * 4..(oy + 1) * ow * 4];
        for (ox, &(x0, x1, fx)) in cols.iter().enumerate() {
            for c in 0..3 {
                let top = u32::from(r0[x0 * 4 + c]) * (256 - fx) + u32::from(r0[x1 * 4 + c]) * fx;
                let bot = u32::from(r1[x0 * 4 + c]) * (256 - fx) + u32::from(r1[x1 * 4 + c]) * fx;
                dst[ox * 4 + c] = ((top * (256 - fy) + bot * fy) >> 16) as u8;
            }
        }
    }
    out
}

/// WinRT errors carry an HRESULT and a system message, never recognized text (PRIV-07).
fn win_err(e: windows::core::Error) -> Error {
    Error::Ocr(format!("Windows OCR: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_conversion_handles_formats_and_stride() {
        // 1x2 RGBA with 4 bytes of row padding.
        let rgba = Frame::new(
            1,
            2,
            8,
            PixelFormat::Rgba8,
            vec![1, 2, 3, 0, 9, 9, 9, 9, 4, 5, 6, 0, 9, 9, 9, 9],
        )
        .unwrap();
        assert_eq!(to_bgra(&rgba), vec![3, 2, 1, 255, 6, 5, 4, 255]);
        let gray = Frame::new(2, 1, 2, PixelFormat::Gray8, vec![7, 8]).unwrap();
        assert_eq!(to_bgra(&gray), vec![7, 7, 7, 255, 8, 8, 8, 255]);
        let bgra = Frame::new(1, 1, 4, PixelFormat::Bgra8, vec![1, 2, 3, 0]).unwrap();
        assert_eq!(to_bgra(&bgra), vec![1, 2, 3, 255]);
    }

    #[test]
    fn bilinear_upscale_keeps_flat_areas_and_interpolates_edges() {
        // 2x1: black | white.
        let src = [0, 0, 0, 255, 255, 255, 255, 255];
        let out = upscale_bilinear(&src, 2, 1, 2);
        assert_eq!(out.len(), 4 * 2 * 4);
        let row: Vec<u8> = out[..16].chunks(4).map(|p| p[0]).collect();
        // Outer pixels keep their colour, inner ones blend.
        assert_eq!(row[0], 0);
        assert_eq!(row[3], 255);
        assert!(
            row[1] > 0 && row[1] < 128 && row[2] > 128 && row[2] < 255,
            "{row:?}"
        );
        assert_eq!(
            &out[..16],
            &out[16..],
            "rows of a 1-px-high image are identical"
        );
        assert_eq!(upscale_bilinear(&src, 2, 1, 1), src);
    }
}
