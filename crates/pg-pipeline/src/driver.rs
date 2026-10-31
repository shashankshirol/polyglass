//! The live loop: feeds lens events into the [`Scheduler`], runs cycles, and emits updates.
//!
//! A new lens position drops the in-flight cycle and bumps the generation, and every result is
//! checked against the current generation before it is emitted, so results for an old
//! position are never drawn (CAP-03). While hidden the loop just waits on its command channel.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use pg_core::PhysRect;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until};

use crate::pipeline::{CycleOutcome, Pipeline};
use crate::scheduler::{Scheduler, SchedulerConfig};
use crate::update::LensUpdate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Show,
    Hide,
    LensMoved(PhysRect),
    Invalidate,
    Shutdown,
}

/// Counters returned when the driver stops (used by LENS-06 / CAP-03 tests and pg-bench).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DriverStats {
    /// Cycles started.
    pub cycles: u64,
    /// Updates sent to the UI.
    pub emitted: u64,
    /// Cycles skipped because the frame was unchanged.
    pub unchanged: u64,
    /// In-flight cycles dropped because the lens moved or settings changed.
    pub cancelled: u64,
    /// Results discarded because their generation was stale.
    pub stale: u64,
}

/// Cheap, cloneable handle for sending lens events to the driver.
#[derive(Debug, Clone)]
pub struct DriverHandle {
    tx: mpsc::UnboundedSender<Command>,
}

impl DriverHandle {
    pub fn show(&self) {
        let _ = self.tx.send(Command::Show);
    }

    pub fn hide(&self) {
        let _ = self.tx.send(Command::Hide);
    }

    /// The lens interior moved or was resized (physical pixels).
    pub fn lens_moved(&self, region: PhysRect) {
        let _ = self.tx.send(Command::LensMoved(region));
    }

    /// Settings changed: recapture with a new generation.
    pub fn invalidate(&self) {
        let _ = self.tx.send(Command::Invalidate);
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(Command::Shutdown);
    }
}

type InFlight = Pin<Box<dyn Future<Output = (u64, CycleOutcome)> + Send>>;

/// Start the driver on the current Tokio runtime. The lens starts hidden.
pub fn spawn(
    pipeline: Arc<Pipeline>,
    cfg: SchedulerConfig,
    updates: mpsc::UnboundedSender<LensUpdate>,
) -> (DriverHandle, JoinHandle<DriverStats>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(run(pipeline, Scheduler::new(cfg), rx, updates));
    (DriverHandle { tx }, task)
}

async fn run(
    pipeline: Arc<Pipeline>,
    mut sched: Scheduler,
    mut rx: mpsc::UnboundedReceiver<Command>,
    updates: mpsc::UnboundedSender<LensUpdate>,
) -> DriverStats {
    let mut stats = DriverStats::default();
    let mut inflight: Option<InFlight> = None;

    loop {
        // Only one cycle at a time; the next deadline is considered once it finishes.
        let deadline = if inflight.is_none() {
            sched.next_deadline()
        } else {
            None
        };

        tokio::select! {
            biased;

            cmd = rx.recv() => {
                let Some(cmd) = cmd else { break };
                let before = sched.generation();
                let now = Instant::now();
                match cmd {
                    Command::Show => sched.show(now),
                    Command::Hide => sched.hide(),
                    Command::LensMoved(r) => sched.lens_moved(r, now),
                    Command::Invalidate => sched.invalidate(now),
                    Command::Shutdown => break,
                }
                if sched.generation() != before && inflight.take().is_some() {
                    stats.cancelled += 1;
                }
            }

            (generation, outcome) = async { inflight.as_mut().expect("guarded").await },
                if inflight.is_some() =>
            {
                inflight = None;
                match outcome {
                    _ if generation != sched.generation() => stats.stale += 1,
                    CycleOutcome::Unchanged => stats.unchanged += 1,
                    CycleOutcome::Update(u) => {
                        stats.emitted += 1;
                        if updates.send(*u).is_err() {
                            break;
                        }
                    }
                }
            }

            () = sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
                if let Some(tick) = sched.poll(Instant::now()) {
                    stats.cycles += 1;
                    let p = pipeline.clone();
                    inflight = Some(Box::pin(async move { (tick.generation, p.run_cycle(tick).await) }));
                }
            }
        }
    }
    stats
}
