//! The Polyglass pipeline: scheduler → capture → change detect → OCR → block group → cache →
//! translate (PLAN §3). Headless: no Tauri dependency, testable with mocks.

pub mod cache;
pub mod change;
pub mod color;
pub mod driver;
pub mod grouping;
mod pipeline;
pub mod scheduler;
pub mod update;

pub use driver::{DriverHandle, DriverStats};
pub use pipeline::{CycleOutcome, LowConfidence, Pipeline, PipelineConfig};
pub use scheduler::{Scheduler, SchedulerConfig, Tick};
pub use update::{BackendInfo, LensRect, LensStatus, LensUpdate, Notice, RenderedBlock, Timings};
