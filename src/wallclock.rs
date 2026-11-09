// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Host wall-clock helpers for card crates that drive RTC chip simulations.
//!
//! Chip crates (e.g. `msm5832`, `mm58174a`) should **not** call functions in
//! this module directly — they should take `wall_secs` as an input parameter
//! so their behavior remains deterministic and snapshot-safe. Only the
//! owning card crate should consult the host clock.
//!
//! The value returned by [`host_local_wall_secs`] is a *pseudo-Unix* seconds
//! count: it is `UTC_unix + local_offset_seconds`, so that feeding it through
//! [`crate::calendar::unix_to_broken`] produces the local wall-clock fields a
//! real RTC chip would expose to the guest OS. It is **not** a true UTC Unix
//! timestamp and should not be compared against `SystemTime::now()` results
//! obtained elsewhere.
//!
//! Within a single session, two `host_local_wall_secs()` values can be
//! compared to compute an offset, and the offset can be added back later to
//! derive a consistent wall-clock view. Across DST transitions the result
//! jumps by ±1 hour — this is accepted as a retrocomputing-appropriate
//! simplification.

use chrono::{Local, Timelike};

/// Returns a pseudo-Unix seconds value for the current host local wall-clock.
///
/// See the module-level docs for the exact semantics.
pub fn host_local_wall_secs() -> i64 {
    let now = Local::now();
    now.timestamp() + now.offset().local_minus_utc() as i64
}

/// Returns `(wall_secs, tenths)` for the current host local wall-clock.
///
/// `tenths` is the sub-second component expressed as tenths of a second
/// (0–9), computed from the current nanosecond. Useful for RTC chips like
/// the MM58174A that expose a tenths-of-seconds register.
pub fn now_local() -> (i64, u32) {
    let now = Local::now();
    let wall = now.timestamp() + now.offset().local_minus_utc() as i64;
    let tenths = now.nanosecond() / 100_000_000;
    (wall, tenths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_local_wall_secs_in_plausible_range() {
        let w = host_local_wall_secs();
        // Between 2020-01-01 and 2100-01-01 roughly (wide sanity window).
        assert!(w > 1_577_836_800, "wall_secs too low: {w}");
        assert!(w < 4_102_444_800, "wall_secs too high: {w}");
    }

    #[test]
    fn now_local_tenths_in_range() {
        let (_, tenths) = now_local();
        assert!(tenths <= 9, "tenths out of range: {tenths}");
    }

    #[test]
    fn now_local_agrees_with_host_local_wall_secs() {
        // Read both in quick succession — should be within 1 second.
        let (w1, _) = now_local();
        let w2 = host_local_wall_secs();
        assert!((w2 - w1).abs() <= 1);
    }
}
