// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Howard M. Harte

//! Pure epoch-based calendar math shared by RTC chip and card crates.
//!
//! This module intentionally has **zero runtime dependencies**. It operates
//! on a *pseudo-Unix* seconds-since-epoch value and converts it to or from
//! broken-down date/time fields. It knows nothing about time zones.
//!
//! To get local wall-clock behavior, pair this module with
//! [`crate::wallclock`], whose `host_local_wall_secs()` returns
//! `UTC_unix + local_offset_seconds`. Feeding
//! that value to [`unix_to_broken`] yields the local wall-clock fields that a
//! real RTC chip would present to the guest OS.
//!
//! RTC chip crates (e.g. `msm5832`, `mm58174a`) use *only* this module and
//! accept a wall-clock seconds value as input — they never touch the host
//! clock directly, which keeps their behavior deterministic and snapshot-safe.
//! The owning card crate is responsible for computing the wall-clock value.

/// Broken-down date/time fields produced by [`unix_to_broken`].
///
/// Calendar conventions:
///
/// - `mon` is **1-based** (January = 1)
/// - `wday` is 0–6 with **Sunday = 0**
/// - `year` is the full year (e.g. 2026, not 126)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrokenTime {
    /// Seconds past the minute (0–59).
    pub sec: u32,
    /// Minutes past the hour (0–59).
    pub min: u32,
    /// Hours past midnight (0–23).
    pub hour: u32,
    /// Day of month (1–31).
    pub mday: u32,
    /// Month of year (1–12).
    pub mon: u32,
    /// Day of week (0=Sunday, 6=Saturday).
    pub wday: u32,
    /// Full year (e.g. 2026).
    pub year: i32,
}

/// Returns true if `y` is a Gregorian leap year.
pub fn is_leap(y: i32) -> bool {
    is_leap_year(i64::from(y))
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Signed Gregorian leap-day tally, used as differences between year starts.
fn leap_days_before(year: i64) -> i64 {
    let previous = year - 1;
    previous.div_euclid(4) - previous.div_euclid(100) + previous.div_euclid(400)
}

/// Month lengths (non-leap-year / leap-year). 0-indexed by month number - 1.
const MONTH_DAYS_NORMAL: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const MONTH_DAYS_LEAP: [i64; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

fn month_days(year: i32) -> &'static [i64; 12] {
    if is_leap(year) {
        &MONTH_DAYS_LEAP
    } else {
        &MONTH_DAYS_NORMAL
    }
}

/// Convert a (pseudo-)Unix seconds value to broken-down fields.
///
/// The result treats `secs` as if it were an unadjusted epoch value, so if
/// the caller passed in a wall-clock-adjusted pseudo-Unix value (see module
/// docs), the returned fields reflect local wall-clock time.
/// Work is bounded by one 400-year Gregorian cycle, even for distant dates.
///
/// # Panics
///
/// Panics if the resulting calendar year cannot be represented by `i32`.
pub fn unix_to_broken(secs: i64) -> BrokenTime {
    let daily = secs.rem_euclid(86400) as u32;
    let mut days = secs.div_euclid(86400);

    let sec = daily % 60;
    let min = (daily / 60) % 60;
    let hour = daily / 3600;

    // 1970-01-01 was a Thursday (wday = 4). Sunday = 0.
    let wday = ((days + 4).rem_euclid(7)) as u32;

    // Leap-year patterns repeat every 400 years: 400 * 365 + 97 days.
    // Euclidean division also places pre-epoch inputs inside a positive cycle.
    let cycles = days.div_euclid(146_097);
    let mut year = 1970 + cycles * 400;
    days = days.rem_euclid(146_097);
    loop {
        let year_days = if is_leap_year(year) { 366 } else { 365 };
        if days < year_days {
            break;
        }
        days -= year_days;
        year += 1;
    }
    let year = i32::try_from(year).expect("calendar year is outside the i32 range");

    // `days` is now the 0-based day-of-year, so subtracting month lengths
    // always settles inside some month; the `12` initializer is only a
    // defensive fallback that the loop overwrites.
    let md = month_days(year);
    let mut mon = 12u32;
    for (i, &len) in md.iter().enumerate() {
        if days < len {
            mon = (i + 1) as u32;
            break;
        }
        days -= len;
    }

    BrokenTime {
        sec,
        min,
        hour,
        mday: days as u32 + 1,
        mon,
        wday,
        year,
    }
}

/// Convert broken-down fields to a (pseudo-)Unix seconds value.
///
/// Inverse of [`unix_to_broken`]. The same epoch semantics apply: if the
/// caller is working in local wall-clock coordinates, the returned value is
/// a wall-clock pseudo-Unix seconds that should be compared against other
/// wall-clock pseudo-Unix values (not against a true UTC Unix timestamp).
///
/// Valid calendar fields round-trip with [`unix_to_broken`]. For RTC register
/// inputs, month zero selects January and months 13 and above select the following
/// January; day zero selects the first day. Larger day/time fields carry into
/// subsequent days. Inputs are not otherwise validated as a civil date.
pub fn broken_to_unix(year: i32, mon: u32, mday: u32, hour: u32, min: u32, sec: u32) -> i64 {
    let mut days =
        (i64::from(year) - 1970) * 365 + leap_days_before(i64::from(year)) - leap_days_before(1970);
    let md = month_days(year);
    for &len in md.iter().take((mon.saturating_sub(1) as usize).min(12)) {
        days += len;
    }
    days += (mday.saturating_sub(1)) as i64;

    days * 86400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_leap_boundary_years() {
        assert!(is_leap(2000)); // divisible by 400
        assert!(!is_leap(1900)); // divisible by 100 but not 400
        assert!(is_leap(2004));
        assert!(!is_leap(2005));
        assert!(!is_leap(2100));
        assert!(is_leap(2400));
    }

    #[test]
    fn round_trip_components() {
        let unix = broken_to_unix(2026, 1, 1, 15, 30, 45);
        let bt = unix_to_broken(unix);
        assert_eq!(bt.year, 2026);
        assert_eq!(bt.mon, 1);
        assert_eq!(bt.mday, 1);
        assert_eq!(bt.hour, 15);
        assert_eq!(bt.min, 30);
        assert_eq!(bt.sec, 45);
    }

    #[test]
    fn round_trip_epoch() {
        let bt = unix_to_broken(0);
        assert_eq!(bt.year, 1970);
        assert_eq!(bt.mon, 1);
        assert_eq!(bt.mday, 1);
        assert_eq!(bt.hour, 0);
        assert_eq!(bt.min, 0);
        assert_eq!(bt.sec, 0);
        assert_eq!(bt.wday, 4); // Thursday
    }

    #[test]
    fn round_trip_leap_day() {
        // Synthetic leap-day fixture at noon in a year divisible by 400.
        let unix = broken_to_unix(2000, 2, 29, 12, 0, 0);
        let bt = unix_to_broken(unix);
        assert_eq!(bt.year, 2000);
        assert_eq!(bt.mon, 2);
        assert_eq!(bt.mday, 29);
        assert_eq!(bt.hour, 12);
    }

    #[test]
    fn day_of_week_known_dates() {
        // 1970-01-01 = Thursday
        let bt = unix_to_broken(0);
        assert_eq!(bt.wday, 4);

        // Placeholder new-year fixture: 2026-01-01 is Thursday.
        let bt = unix_to_broken(broken_to_unix(2026, 1, 1, 0, 0, 0));
        assert_eq!(bt.wday, 4, "placeholder new year should be Thursday");
    }

    #[test]
    fn broken_to_unix_consistent_units() {
        // +1 hour ≡ +3600 seconds
        let a = broken_to_unix(2000, 6, 15, 12, 0, 0);
        let b = broken_to_unix(2000, 6, 15, 13, 0, 0);
        assert_eq!(b - a, 3600);

        // +1 day ≡ +86400 seconds
        let c = broken_to_unix(2000, 6, 16, 12, 0, 0);
        assert_eq!(c - a, 86400);
    }

    #[test]
    fn pre_epoch_dates_accumulate_negative_seconds() {
        // Years before 1970 exercise the negative-day accumulation branch
        // of broken_to_unix.
        assert_eq!(broken_to_unix(1969, 12, 31, 0, 0, 0), -86400);
        assert_eq!(broken_to_unix(1969, 12, 30, 0, 0, 0), -2 * 86400);
        // 1969 is a common year: exactly 365 days before the epoch.
        assert_eq!(broken_to_unix(1969, 1, 1, 0, 0, 0), -365 * 86400);
        // Crossing the 1968 leap year goes strictly further negative.
        assert!(broken_to_unix(1968, 1, 1, 0, 0, 0) < broken_to_unix(1969, 1, 1, 0, 0, 0));
    }

    #[test]
    fn pre_epoch_round_trip() {
        // unix_to_broken must handle negative timestamps (pre-1970) without
        // overflowing, and round-trip cleanly with broken_to_unix.
        let cases = [
            (1969, 12, 31, 23, 59, 59),
            (1969, 1, 1, 0, 0, 0),
            (1968, 2, 29, 12, 0, 0), // leap day before the epoch
            (1900, 1, 1, 0, 0, 0),   // common year (÷100, not ÷400)
            (1601, 6, 15, 6, 30, 0),
        ];
        for (year, mon, mday, hour, min, sec) in cases {
            let unix = broken_to_unix(year, mon, mday, hour, min, sec);
            assert!(unix < 0, "{year}-{mon:02}-{mday:02} should be pre-epoch");
            let bt = unix_to_broken(unix);
            assert_eq!(bt.year, year, "year for {unix}");
            assert_eq!(bt.mon, mon, "mon for {unix}");
            assert_eq!(bt.mday, mday, "mday for {unix}");
            assert_eq!(bt.hour, hour, "hour for {unix}");
            assert_eq!(bt.min, min, "min for {unix}");
            assert_eq!(bt.sec, sec, "sec for {unix}");
        }
    }

    #[test]
    fn pre_epoch_weekday_is_correct() {
        // The day before the Unix epoch is Wednesday (wday = 3).
        let bt = unix_to_broken(broken_to_unix(1969, 12, 31, 0, 0, 0));
        assert_eq!(bt.wday, 3, "the day before the epoch should be Wednesday");
    }

    #[test]
    fn december_31_does_not_overflow_month() {
        let unix = broken_to_unix(2025, 12, 31, 23, 59, 59);
        let bt = unix_to_broken(unix);
        assert_eq!(bt.year, 2025);
        assert_eq!(bt.mon, 12);
        assert_eq!(bt.mday, 31);
        assert_eq!(bt.hour, 23);
        assert_eq!(bt.min, 59);
        assert_eq!(bt.sec, 59);
    }

    #[test]
    fn distant_and_negative_years_round_trip_at_month_boundaries() {
        for year in [
            i32::MIN,
            -4000,
            -1,
            0,
            1,
            1969,
            1970,
            2000,
            1_000_000,
            i32::MAX,
        ] {
            for mon in 1..=12 {
                for day in [1, month_days(year)[mon as usize - 1] as u32] {
                    let seconds = broken_to_unix(year, mon, day, 23, 59, 59);
                    let date = unix_to_broken(seconds);
                    assert_eq!((date.year, date.mon, date.mday), (year, mon, day));
                    assert_eq!((date.hour, date.min, date.sec), (23, 59, 59));
                }
            }
        }
    }

    #[test]
    fn calendar_math_agrees_with_an_independent_chrono_oracle() {
        use chrono::Datelike;

        for year in [
            -200_000, -4000, -1, 0, 1, 1600, 1900, 1970, 2000, 2026, 2400, 200_000,
        ] {
            for month in 1..=12 {
                let day = month_days(year)[month as usize - 1] as u32;
                let reference = chrono::NaiveDate::from_ymd_opt(year, month, day).unwrap();
                let seconds = reference
                    .and_hms_opt(23, 59, 59)
                    .unwrap()
                    .and_utc()
                    .timestamp();
                assert_eq!(broken_to_unix(year, month, day, 23, 59, 59), seconds);
                let converted = unix_to_broken(seconds);
                assert_eq!(
                    (converted.year, converted.mon, converted.mday),
                    (year, month, day)
                );
                assert_eq!(converted.wday, reference.weekday().num_days_from_sunday());
            }
        }
    }

    #[test]
    fn rtc_register_normalization_is_preserved() {
        assert_eq!(
            broken_to_unix(2026, 0, 0, 0, 0, 0),
            broken_to_unix(2026, 1, 1, 0, 0, 0)
        );
        assert_eq!(
            broken_to_unix(2026, 99, 1, 0, 0, 0),
            broken_to_unix(2027, 1, 1, 0, 0, 0)
        );
        assert_eq!(
            broken_to_unix(2026, 1, 1, 24, 0, 0),
            broken_to_unix(2026, 1, 2, 0, 0, 0)
        );
    }

    #[test]
    #[should_panic(expected = "calendar year is outside the i32 range")]
    fn unrepresentable_future_year_fails_without_an_unbounded_search() {
        unix_to_broken(i64::MAX);
    }

    #[test]
    #[should_panic(expected = "calendar year is outside the i32 range")]
    fn unrepresentable_past_year_fails_without_an_unbounded_search() {
        unix_to_broken(i64::MIN);
    }
}
