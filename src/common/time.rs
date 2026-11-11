//! RFC 3339 timestamp helpers shared by all services.
//!
//! Timestamps are always produced in UTC and stored as strings (SQLite has no
//! native datetime type); comparison helpers parse them back with `time`.

use jiff::tz::TimeZone;
use jiff::Timestamp;
use std::sync::OnceLock;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

/// The `Europe/Paris` time zone served by jiff's embedded tz database (the
/// runtime image ships no zoneinfo); falls back to UTC if the id cannot be
/// resolved.
pub fn paris() -> &'static TimeZone {
    static PARIS: OnceLock<TimeZone> = OnceLock::new();
    PARIS.get_or_init(|| TimeZone::get("Europe/Paris").unwrap_or(TimeZone::UTC))
}

/// Wall-clock minute of the day (0..1440) in Europe/Paris at `ts` — the unit [`crate::services::bot::quiet_hours::QuietHours`] compares against.
pub fn paris_minutes_at(ts: Timestamp) -> u16 {
    let zoned = ts.to_zoned(paris().clone());
    // jiff invariants: hour is 0..=23, minute is 0..=59.
    u16::from(zoned.hour() as u8) * 60 + u16::from(zoned.minute() as u8)
}

/// [`paris_minutes_at`] for the current instant.
pub fn paris_minutes_now() -> u16 {
    paris_minutes_at(Timestamp::now())
}

/// The current UTC time, formatted RFC 3339 — the canonical way `created_at`/
/// `expires_at`-style columns are filled. (Only a real clock failure could
/// make this panic.)
pub fn now() -> String {
    OffsetDateTime::now_utc().format(&Rfc3339).unwrap()
}

/// The UTC time `days` days from now, formatted RFC 3339 — e.g. session
/// expiries. Negative values are accepted (goes backwards in time).
pub fn in_days(days: i64) -> String {
    (OffsetDateTime::now_utc() + Duration::days(days))
        .format(&Rfc3339)
        .unwrap()
}

/// The UTC time `minutes` minutes from now, formatted RFC 3339 — short-lived
/// deadlines such as Telegram link tokens.
pub fn in_minutes(minutes: i64) -> String {
    (OffsetDateTime::now_utc() + Duration::minutes(minutes))
        .format(&Rfc3339)
        .unwrap()
}

/// Whether an RFC 3339 timestamp is already in the past.
///
/// Unparseable input counts as past (fail-closed): an expired or corrupted
/// deadline should behave like an expired one, never like a live one.
pub fn is_past(rfc3339: &str) -> bool {
    match OffsetDateTime::parse(rfc3339, &Rfc3339) {
        Ok(t) => OffsetDateTime::now_utc() > t,
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paris wall-clock minute of the day at a fixed UTC instant.
    fn minutes_at(instant: &str) -> u16 {
        paris_minutes_at(instant.parse::<Timestamp>().expect("valid instant"))
    }

    // The minute count is a pure function of the instant: pin it, never read the clock.
    #[test]
    fn paris_minutes_at_reads_the_paris_wall_clock() {
        assert_eq!(minutes_at("2026-01-15T22:59:00Z"), 23 * 60 + 59); // 23:59 CET (+1)
        assert_eq!(minutes_at("2026-07-15T21:00:00Z"), 23 * 60); // 23:00 CEST (+2)
        assert_eq!(minutes_at("2026-01-15T23:00:00Z"), 0); // midnight is 0, not 1440
        assert_eq!(minutes_at("2026-01-15T23:30:00Z"), 30); // day wrap
    }

    // European DST: 60 real minutes can span 120 local minutes, and 02:30 happens twice.
    #[test]
    fn paris_minutes_at_follows_dst_transitions() {
        assert_eq!(minutes_at("2026-03-29T00:30:00Z"), 90); // 01:30 CET
        assert_eq!(minutes_at("2026-03-29T01:30:00Z"), 210); // 03:30 CEST
        assert_eq!(minutes_at("2026-10-25T00:30:00Z"), 150); // 02:30 CEST
        assert_eq!(minutes_at("2026-10-25T01:30:00Z"), 150); // 02:30 CET
    }

    // `paris()` degrades to UTC when the embedded tzdb is unusable; pin both offsets.
    #[test]
    fn paris_never_silently_falls_back_to_utc() {
        let offset = |instant: &str| {
            let ts = instant.parse::<Timestamp>().expect("valid instant");
            paris().to_offset(ts).seconds()
        };
        assert_eq!(offset("2026-01-15T12:00:00Z"), 3_600);
        assert_eq!(offset("2026-07-15T12:00:00Z"), 7_200);
    }

    // Independent oracle: UTC wall clock plus the resolved offset, wrapped in a day.
    #[test]
    fn paris_minutes_at_agrees_with_offset_arithmetic() {
        for instant in [
            "2026-01-15T22:59:00Z",
            "2026-03-29T01:30:00Z",
            "2026-10-25T01:30:00Z",
            "2026-07-15T21:00:00Z",
        ] {
            let ts = instant.parse::<Timestamp>().expect("valid instant");
            let utc = ts.to_zoned(TimeZone::UTC);
            let utc_minutes = u16::from(utc.hour() as u8) * 60 + u16::from(utc.minute() as u8);
            let offset_minutes = paris().to_offset(ts).seconds() / 60;
            let expected = (i32::from(utc_minutes) + offset_minutes).rem_euclid(1_440) as u16;
            assert_eq!(paris_minutes_at(ts), expected, "{instant}");
        }
    }

    // The clock cannot be pinned: assert the range invariant only.
    #[test]
    fn paris_minutes_now_stays_within_a_day() {
        assert!(paris_minutes_now() < 1_440);
    }
}
