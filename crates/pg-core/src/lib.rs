//! Shared types used by every Polyglass crate.
//!
//! This crate has no platform or Tauri dependencies (PLAN principle 4).

mod error;
mod frame;
mod geometry;
mod lang;
mod text;

pub use error::{Error, Result};
pub use frame::{Frame, PixelFormat};
pub use geometry::{LogicalRect, PhysRect, Rect};
pub use lang::{LangPair, LangTag, LangTagError, SourceLang};
pub use text::{Block, Line};
