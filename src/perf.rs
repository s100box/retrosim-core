// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Per-frame wall-time vs simulated-time accounting.
//!
//! Retro simulators typically want two things the moment they grow a
//! non-trivial CPU emulator:
//!
//! 1. **Telemetry** — "is the sim keeping up with real hardware?", as
//!    a `speed=X.XXx, cpu=XX%, fps=XX` line once per wall-second,
//!    surfaced in the window title or TUI banner.
//! 2. **Wall-proportional pacing** — each CPU-driving call (the
//!    platform's `step_frame` / `run_until_break`) receives a
//!    sim-cycle budget sized to the wall time elapsed since the
//!    previous call, so sim-time matches wall-time 1:1 even when the
//!    runner calls us less often than 60 Hz (e.g. a slow render path
//!    throttles redraws to 20 Hz — each call then advances 3× the
//!    per-frame budget so total sim progress still equals real time).
//!
//! Both concerns decouple the emulator-speed question from the
//! render-cadence question.  [`PerfTracker`] handles (1);
//! [`WallProportionalBudget`] handles (2).  Neither touches I/O or
//! winit; they're plain data + arithmetic suitable for any sim that
//! uses the retrosim stack.

use std::time::{Duration, Instant};

/// Rolling wall-time vs sim-time accumulator.
///
/// Call [`begin_frame`](Self::begin_frame) at the top of the
/// CPU-driving function to record a wall timestamp, then
/// [`end_frame`](Self::end_frame) at the bottom with the number of
/// simulated cycles consumed.  Once per wall-second the tracker
/// formats `"speed=X.XXx, cpu=XX%, fps=XX"` and retains it on
/// [`status`](Self::status); UI surfaces (window title, TUI banner,
/// etc.) read that without re-formatting between reports.
///
/// `speed = sim_s / wall_s` where both accumulate only inside the
/// CPU loop.  `speed ≥ 1.0` means the emulator is keeping up;
/// speed-within-active × cpu-busy-fraction gives the net real-time
/// ratio (also printed as `sim X.XXs / wall X.XXs` in the debug log
/// produced by the caller).
pub struct PerfTracker {
    /// Simulated CPU clock (Hz) used to convert `sim_cycles` into
    /// `sim_s`.
    sim_hz: u32,
    /// Wall time spent inside the CPU loop since the last report.
    wall: Duration,
    /// Simulated CPU cycles advanced since the last report.
    sim_cycles: u64,
    /// Count of [`end_frame`] calls since the last report.
    frames: u32,
    /// Timestamp of the last status emit (set on the first frame).
    last_report: Option<Instant>,
    /// Latest formatted status string — retained so UI layers can
    /// display it continuously between per-second updates.
    status: Option<String>,
    /// Latest raw metrics for callers that want to log their own
    /// line without re-parsing the formatted string.
    snapshot: Option<PerfSnapshot>,
}

/// Raw metrics captured at the last per-second report point.
#[derive(Debug, Clone, Copy)]
pub struct PerfSnapshot {
    /// Ratio of sim-time advanced to wall-time spent inside the CPU
    /// loop.  1.0 = real-time; > 1.0 = emulator has headroom;
    /// < 1.0 = emulator is the bottleneck.
    pub speed: f64,
    /// Fraction of the report window (wall-seconds) spent inside
    /// the CPU loop.  0.05 = 5 % CPU-busy.
    pub cpu_busy: f64,
    /// Frame-driving calls per wall-second.
    pub fps: f64,
    /// Simulated seconds advanced over the report window.
    pub sim_s: f64,
    /// Wall seconds spent inside the CPU loop over the report
    /// window.
    pub wall_s: f64,
}

impl PerfTracker {
    /// Construct a tracker for a CPU running at `sim_hz`.  The
    /// value is used solely to normalize `sim_cycles` into seconds
    /// for the `speed` ratio — it does not drive any pacing.
    pub fn new(sim_hz: u32) -> Self {
        Self {
            sim_hz,
            wall: Duration::ZERO,
            sim_cycles: 0,
            frames: 0,
            last_report: None,
            status: None,
            snapshot: None,
        }
    }

    /// Convenience alias for `Instant::now()` at the top of a frame.
    /// Callers pass the returned value back into [`end_frame`](Self::end_frame).
    pub fn begin_frame() -> Instant {
        Instant::now()
    }

    /// Record that a CPU-driving call ran for `wall_start.elapsed()`
    /// wall time and advanced `sim_cycles_consumed` simulated cycles.
    /// When at least one wall-second has elapsed since the report window began, format a
    /// new status string and return `Some(PerfSnapshot)`.  Callers
    /// typically discard the return and just query [`status`](Self::status) or
    /// [`snapshot`](Self::snapshot), but the return value is there for callers that
    /// want to log a bespoke line on the same tick.
    pub fn end_frame(
        &mut self,
        wall_start: Instant,
        sim_cycles_consumed: u64,
    ) -> Option<PerfSnapshot> {
        self.record_frame(wall_start.elapsed(), sim_cycles_consumed, Instant::now())
    }

    /// Core of [`end_frame`](Self::end_frame) with the wall clock
    /// injected.  `frame_wall` is the wall time the just-finished frame
    /// spent in the CPU loop and `now` is the current instant used to
    /// gate the per-second report.  Injecting the clock keeps the report
    /// boundary testable without sleeping on the real clock.
    fn record_frame(
        &mut self,
        frame_wall: Duration,
        sim_cycles_consumed: u64,
        now: Instant,
    ) -> Option<PerfSnapshot> {
        self.wall += frame_wall;
        self.sim_cycles = self.sim_cycles.saturating_add(sim_cycles_consumed);
        self.frames = self.frames.saturating_add(1);

        let last = *self.last_report.get_or_insert(now);
        let window = now.duration_since(last).as_secs_f64();
        if window < 1.0 {
            return None;
        }
        let wall_s = self.wall.as_secs_f64();
        let snap = if wall_s > 0.0 {
            let sim_s = self.sim_cycles as f64 / self.sim_hz as f64;
            let speed = sim_s / wall_s;
            let cpu_busy = wall_s / window;
            let fps = self.frames as f64 / window;
            let status = format!(
                "speed={:.2}x, cpu={:.0}%, fps={:.0}",
                speed,
                cpu_busy * 100.0,
                fps
            );
            self.status = Some(status);
            let snap = PerfSnapshot {
                speed,
                cpu_busy,
                fps,
                sim_s,
                wall_s,
            };
            self.snapshot = Some(snap);
            Some(snap)
        } else {
            None
        };
        self.wall = Duration::ZERO;
        self.sim_cycles = 0;
        self.frames = 0;
        self.last_report = Some(now);
        snap
    }

    /// Latest formatted status, or `None` until the first wall-second
    /// of data has been reported.
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Raw metrics from the last report, useful when a caller wants
    /// to emit its own log line rather than display the formatted
    /// string.
    pub fn snapshot(&self) -> Option<PerfSnapshot> {
        self.snapshot
    }

    /// Simulated CPU clock (Hz) this tracker was constructed with.
    pub fn sim_hz(&self) -> u32 {
        self.sim_hz
    }
}

/// Sim-cycle budget sized to the wall time elapsed between CPU-
/// driving calls, so sim-time matches wall-time 1:1 regardless of
/// how often the outer runner calls us.
///
/// Call [`budget`](Self::budget) at the top of each `step_frame` /
/// `run_until_break` with the current `Instant`.  The returned
/// u64 is the number of sim cycles to advance before returning.
/// The first call uses `frame_cycles` (no prior timestamp to
/// measure against), subject to the same clamp as later calls. Subsequent
/// calls scale with the wall gap, clamped to `[min_budget, max_budget]` — small enough to keep
/// interactive cadence snappy, large enough to absorb slow renders
/// without falling behind real time.
pub struct WallProportionalBudget {
    /// Simulated CPU clock (Hz) used to convert wall time into
    /// cycles.
    sim_hz: u32,
    /// Cycles per "one frame's worth" at the target FPS.  Also the
    /// default budget for the first call, and the clamp floor.
    frame_cycles: u64,
    /// Minimum returned budget.  Defaults to `frame_cycles` so
    /// interactive response never drops below one frame's worth.
    min_budget: u64,
    /// Maximum returned budget.  Defaults to `frame_cycles * 8` so
    /// a long wall gap (debugger pause, laptop sleep) can't trigger
    /// a multi-minute run-ahead catch-up on resume.
    max_budget: u64,
    /// Wall timestamp of the previous call.
    last_wall: Option<Instant>,
}

impl WallProportionalBudget {
    /// Construct with default clamps of `[frame_cycles,
    /// frame_cycles × 8]`.
    pub fn new(sim_hz: u32, frame_cycles: u64) -> Self {
        Self {
            sim_hz,
            frame_cycles,
            min_budget: frame_cycles,
            max_budget: frame_cycles.saturating_mul(8),
            last_wall: None,
        }
    }

    /// Construct with explicit clamp bounds (in units of frames).
    /// `min_frames = 1, max_frames = 8` reproduces [`new`](Self::new).
    ///
    /// # Panics
    ///
    /// Panics if `min_frames` exceeds `max_frames`.
    pub fn with_bounds(sim_hz: u32, frame_cycles: u64, min_frames: u32, max_frames: u32) -> Self {
        assert!(
            min_frames <= max_frames,
            "min_frames must not exceed max_frames"
        );
        Self {
            sim_hz,
            frame_cycles,
            min_budget: frame_cycles.saturating_mul(min_frames as u64),
            max_budget: frame_cycles.saturating_mul(max_frames as u64),
            last_wall: None,
        }
    }

    /// Compute the sim-cycle budget for a CPU-driving call that
    /// starts at `wall_now`, and update internal state so the next
    /// call measures its gap relative to this one.
    pub fn budget(&mut self, wall_now: Instant) -> u64 {
        let raw = match self.last_wall {
            Some(prev) => {
                let elapsed = wall_now.saturating_duration_since(prev).as_secs_f64();
                (elapsed * self.sim_hz as f64) as u64
            }
            None => self.frame_cycles,
        };
        self.last_wall = Some(wall_now);
        raw.clamp(self.min_budget, self.max_budget)
    }

    /// Frame-cycle unit this budget was constructed with.
    pub fn frame_cycles(&self) -> u64 {
        self.frame_cycles
    }

    /// Simulated CPU clock (Hz).
    pub fn sim_hz(&self) -> u32 {
        self.sim_hz
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_emits_no_status_before_first_second() {
        let mut t = PerfTracker::new(1_000_000);
        let start = Instant::now();
        for frame in 0..10 {
            let now = start + Duration::from_millis(frame * 10);
            assert!(t
                .record_frame(Duration::from_millis(1), 16_666, now)
                .is_none());
        }
        assert!(t.status().is_none());
        assert!(t.snapshot().is_none());
    }

    #[test]
    fn tracker_emits_status_once_past_one_second() {
        let mut t = PerfTracker::new(1_000_000);
        let t0 = Instant::now();
        // First frame seeds `last_report`; window is zero, so no report.
        assert!(t.record_frame(Duration::from_millis(5), 0, t0).is_none());

        // A frame whose `now` is >= 1s past the seed crosses the boundary.
        let t1 = t0 + Duration::from_millis(1050);
        let snap = t.record_frame(Duration::from_millis(5), 1_000_000, t1);
        assert!(snap.is_some());
        let snap = snap.unwrap();
        assert!(snap.speed > 0.0);
        assert!(snap.wall_s > 0.0);
        let s = t.status().unwrap();
        assert!(s.contains("speed="));
        assert!(s.contains("cpu="));
        assert!(s.contains("fps="));
        // The window reset, so a subsequent sub-second frame reports nothing.
        let t2 = t1 + Duration::from_millis(10);
        assert!(t
            .record_frame(Duration::from_millis(5), 1_000, t2)
            .is_none());
    }

    #[test]
    fn tracker_with_zero_wall_time_emits_no_snapshot() {
        // Even past the one-second window, a report with zero accumulated
        // CPU-loop wall time can't compute a speed ratio, so no snapshot.
        let mut t = PerfTracker::new(1_000_000);
        let t0 = Instant::now();
        assert!(t.record_frame(Duration::ZERO, 0, t0).is_none());
        let t1 = t0 + Duration::from_millis(1100);
        assert!(t.record_frame(Duration::ZERO, 1_000_000, t1).is_none());
        assert!(t.status().is_none());
        assert!(t.snapshot().is_none());
    }

    #[test]
    fn tracker_reports_sim_hz() {
        assert_eq!(PerfTracker::new(4_000_000).sim_hz(), 4_000_000);
    }

    #[test]
    fn budget_first_call_returns_frame_cycles() {
        let mut b = WallProportionalBudget::new(1_000_000, 16_666);
        let budget = b.budget(Instant::now());
        assert_eq!(budget, 16_666);
    }

    #[test]
    fn budget_scales_with_wall_gap() {
        let mut b = WallProportionalBudget::new(1_000_000, 1_000);
        let t0 = Instant::now();
        b.budget(t0);
        // ~100 ms of wall time → 100_000 sim cycles, clamped by the
        // frame_cycles × 8 = 8_000 ceiling.  We therefore expect the
        // ceiling back.
        let t1 = t0 + Duration::from_millis(100);
        let budget = b.budget(t1);
        assert_eq!(budget, 8_000);
    }

    #[test]
    fn budget_floors_at_one_frame() {
        let mut b = WallProportionalBudget::new(1_000_000, 16_666);
        let t0 = Instant::now();
        b.budget(t0);
        // Zero elapsed — raw budget would be 0, but floor is
        // frame_cycles.
        let budget = b.budget(t0);
        assert_eq!(budget, 16_666);
    }

    #[test]
    fn budget_exposes_construction_params() {
        let b = WallProportionalBudget::new(2_000_000, 12_345);
        assert_eq!(b.frame_cycles(), 12_345);
        assert_eq!(b.sim_hz(), 2_000_000);
    }

    #[test]
    fn budget_with_bounds_honours_explicit_clamps() {
        let mut b = WallProportionalBudget::with_bounds(1_000_000, 1_000, 2, 4);
        // Both the initial fallback and measured budgets honor these bounds.
        let t0 = Instant::now();
        assert_eq!(b.budget(t0), 2_000);
        let t1 = t0 + Duration::from_millis(50);
        // 50 ms → 50_000 raw cycles, clamped to ceiling 4 × 1_000
        // = 4_000.
        let budget = b.budget(t1);
        assert_eq!(budget, 4_000);
    }

    #[test]
    #[should_panic(expected = "min_frames must not exceed max_frames")]
    fn reversed_budget_bounds_are_rejected_during_construction() {
        WallProportionalBudget::with_bounds(1_000_000, 1_000, 4, 2);
    }
}
