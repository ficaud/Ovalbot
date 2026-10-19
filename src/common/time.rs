//! RFC 3339 timestamp helpers shared by all services.
//!
//! Timestamps are always produced in UTC and stored as strings (SQLite has no
//! native datetime type); comparison helpers parse them back with `time`.

use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

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
