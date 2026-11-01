//! When to capture (CAP-03) and when to do nothing at all (LENS-06).
//!
//! A pure state machine: callers feed it events with timestamps and ask for the next deadline,
//! so it is fully testable without real time.

use std::time::Duration;

use pg_core::PhysRect;
use tokio::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerConfig {
    /// The lens is "at rest" after not moving for this long (SPEC §2: 150 ms).
    pub rest_delay: Duration,
    /// Re-check interval while at rest (CAP-03: 4 Hz).
    pub recheck_interval: Duration,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            rest_delay: Duration::from_millis(150),
            recheck_interval: Duration::from_millis(250),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Hidden or no region yet: no timers, no capture.
    Parked,
    Moving {
        last_move: Instant,
    },
    Resting {
        next_capture: Instant,
    },
}

/// A capture the driver should run now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    pub generation: u64,
    pub region: PhysRect,
    /// First capture after coming to rest (or after an invalidation): process even if the frame
    /// looks unchanged, because the lens content was cleared when it moved.
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct Scheduler {
    cfg: SchedulerConfig,
    visible: bool,
    region: Option<PhysRect>,
    generation: u64,
    phase: Phase,
    force_next: bool,
}

impl Scheduler {
    pub fn new(cfg: SchedulerConfig) -> Self {
        Self {
            cfg,
            visible: false,
            region: None,
            generation: 0,
            phase: Phase::Parked,
            force_next: true,
        }
    }

    /// Results tagged with an older generation must be dropped (CAP-03 stale-result rule).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn is_parked(&self) -> bool {
        self.phase == Phase::Parked
    }

    pub fn is_moving(&self) -> bool {
        matches!(self.phase, Phase::Moving { .. })
    }

    pub fn region(&self) -> Option<PhysRect> {
        self.region
    }

    /// Capture now if the lens was hidden. Already shown: nothing changes, so a `show` that
    /// follows a move doesn't cut the rest delay short (CAP-03).
    pub fn show(&mut self, now: Instant) {
        if self.visible {
            return;
        }
        self.visible = true;
        self.generation += 1;
        self.rest_now(now);
    }

    /// Park completely: no further ticks until shown again (LENS-06, RES-01).
    pub fn hide(&mut self) {
        self.visible = false;
        self.generation += 1;
        self.phase = Phase::Parked;
    }

    /// The lens moved or was resized. Cancels in-flight work and waits for rest.
    pub fn lens_moved(&mut self, region: PhysRect, now: Instant) {
        self.region = Some(region);
        self.generation += 1;
        self.force_next = true;
        if self.visible && !region.is_empty() {
            self.phase = Phase::Moving { last_move: now };
        } else {
            self.phase = Phase::Parked;
        }
    }

    /// Settings changed (language, backend): recapture now with a new generation.
    pub fn invalidate(&mut self, now: Instant) {
        self.generation += 1;
        if !self.is_moving() {
            self.rest_now(now);
        }
    }

    fn rest_now(&mut self, now: Instant) {
        self.force_next = true;
        self.phase = match self.region {
            Some(r) if self.visible && !r.is_empty() => Phase::Resting { next_capture: now },
            _ => Phase::Parked,
        };
    }

    /// When the driver should call [`Scheduler::poll`] next; `None` = sleep until an event.
    pub fn next_deadline(&self) -> Option<Instant> {
        match self.phase {
            Phase::Parked => None,
            Phase::Moving { last_move } => Some(last_move + self.cfg.rest_delay),
            Phase::Resting { next_capture } => Some(next_capture),
        }
    }

    /// Advance to `now`; returns a capture to run, if one is due.
    pub fn poll(&mut self, now: Instant) -> Option<Tick> {
        let region = self.region?;
        match self.phase {
            Phase::Parked => None,
            Phase::Moving { last_move } if now >= last_move + self.cfg.rest_delay => {
                // Came to rest: capture immediately.
                self.phase = Phase::Resting { next_capture: now };
                self.poll(now)
            }
            Phase::Moving { .. } => None,
            Phase::Resting { next_capture } if now >= next_capture => {
                self.phase = Phase::Resting {
                    next_capture: now + self.cfg.recheck_interval,
                };
                let force = std::mem::replace(&mut self.force_next, false);
                Some(Tick {
                    generation: self.generation,
                    region,
                    force,
                })
            }
            Phase::Resting { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: PhysRect = PhysRect::new(0, 0, 600, 300);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn hidden_scheduler_is_parked() {
        let t0 = Instant::now();
        let mut s = Scheduler::new(SchedulerConfig::default());
        s.lens_moved(R, t0);
        assert!(s.is_parked());
        assert_eq!(s.next_deadline(), None);
        assert_eq!(s.poll(t0 + ms(10_000)), None);
    }

    #[test]
    fn no_capture_while_moving_then_capture_at_rest() {
        let t0 = Instant::now();
        let mut s = Scheduler::new(SchedulerConfig::default());
        s.show(t0);
        s.lens_moved(R, t0);
        for i in 1..10 {
            // Keeps moving every 50 ms: never at rest.
            assert_eq!(s.poll(t0 + ms(i * 50 - 1)), None);
            s.lens_moved(R, t0 + ms(i * 50));
        }
        let last = t0 + ms(450);
        assert_eq!(s.next_deadline(), Some(last + ms(150)));
        assert_eq!(s.poll(last + ms(149)), None);
        let tick = s.poll(last + ms(150)).unwrap();
        assert!(tick.force);
        assert_eq!(tick.generation, s.generation());
    }

    #[test]
    fn rechecks_at_4hz_without_force() {
        let t0 = Instant::now();
        let mut s = Scheduler::new(SchedulerConfig::default());
        s.lens_moved(R, t0);
        s.show(t0);
        assert!(s.poll(t0).unwrap().force);
        assert_eq!(s.poll(t0 + ms(249)), None);
        let t = s.poll(t0 + ms(250)).unwrap();
        assert!(!t.force);
        assert_eq!(s.next_deadline(), Some(t0 + ms(500)));
    }

    #[test]
    fn moving_bumps_generation() {
        let t0 = Instant::now();
        let mut s = Scheduler::new(SchedulerConfig::default());
        s.lens_moved(R, t0);
        s.show(t0);
        let tick = s.poll(t0).unwrap();
        s.lens_moved(PhysRect::new(10, 0, 600, 300), t0 + ms(5));
        assert!(tick.generation < s.generation());
    }

    #[test]
    fn hide_parks_and_show_recaptures() {
        let t0 = Instant::now();
        let mut s = Scheduler::new(SchedulerConfig::default());
        s.lens_moved(R, t0);
        s.show(t0);
        s.poll(t0).unwrap();
        s.hide();
        assert_eq!(s.next_deadline(), None);
        assert_eq!(s.poll(t0 + ms(1000)), None);
        s.show(t0 + ms(2000));
        assert!(s.poll(t0 + ms(2000)).unwrap().force);
    }

    #[test]
    fn show_while_shown_keeps_the_rest_delay() {
        let t0 = Instant::now();
        let mut s = Scheduler::new(SchedulerConfig::default());
        s.show(t0);
        s.lens_moved(R, t0);
        let generation = s.generation();
        s.show(t0 + ms(10));
        assert_eq!(s.generation(), generation);
        assert_eq!(s.poll(t0 + ms(10)), None, "still moving");
        assert_eq!(s.next_deadline(), Some(t0 + ms(150)));
        assert!(s.poll(t0 + ms(150)).is_some());
    }

    #[test]
    fn invalidate_forces_immediate_capture() {
        let t0 = Instant::now();
        let mut s = Scheduler::new(SchedulerConfig::default());
        s.lens_moved(R, t0);
        s.show(t0);
        s.poll(t0).unwrap();
        let g = s.generation();
        s.invalidate(t0 + ms(10));
        let tick = s.poll(t0 + ms(10)).unwrap();
        assert!(tick.force);
        assert!(tick.generation > g);
    }
}
