//! Quiet hours module
//!
//! Takes env variables QUIET_HOURS_START and QUIET_HOURS_END
//! to be able to determine a window within the bot no longer
//! try to purchase tickets because the user is likely no awake
//!
//! The bot still send notification though.
use crate::common::time::{paris_minutes_at, paris_minutes_now};
use jiff::Timestamp;

// ============================== Constants declaration  ==============================
// Maximum minutes in 24 hours (that represents a full day)
const MAX_MINUTES_FROM_MIDNIGHT: u16 = 1440;

/// The window used when the environment leaves quiet hours unset or invalid:
/// 23:00 → 08:00 Paris.
pub const DEFAULT: QuietHours = QuietHours {
    start: 23 * 60,
    end: 8 * 60,
};

// ============================== Phase change detection ==============================

/// What changed between two consecutive loop ticks regarding the quiet
/// window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuietChange {
    /// The previous tick was outside the window, the current one is inside.
    Started,
    /// The previous tick was inside the window, the current one is outside.
    Ended,
}

/// Detects a phase change between two consecutive loop ticks.
///
/// The loop owns the previous state (no global, no mutable field on
/// [`QuietHours`], which stays a `Copy` value); this function is the pure
/// decision core.
///
/// # Arguments
/// * `previous` - window state at the previous tick; `None` at loop startup
/// * `current` - window state at the current tick
///
/// # Returns
/// `Some(QuietChange::Started)` when the bot just entered the window,
/// `Some(QuietChange::Ended)` when it just left, `None` otherwise — including
/// the startup tick, which never reports a transition.
pub fn phase_change(previous: Option<bool>, current: bool) -> Option<QuietChange> {
    match (previous, current) {
        (Some(false), true) => Some(QuietChange::Started),
        (Some(true), false) => Some(QuietChange::Ended),
        _ => None,
    }
}

// ============================== Errors implementation ==============================
/// All errors that the quiet hours can return
#[derive(Debug, PartialEq)]
pub enum QuietHoursErrors {
    /// A bound is not a minute of the day (`0..1440`): midnight is `0`, never
    /// `1440`.
    BornsTooHigh,
    /// `start < end`, i.e. a window that does not wrap midnight — the exact
    /// opposite of what this window means here.
    StartInferiorToEnd,
    /// `start == end`: a zero-length window. Left unchecked it would read as
    /// "quiet all day", which is always a configuration mistake.
    EmptyWindow,
}

/// Display implementation for QuietHoursErrors
impl std::fmt::Display for QuietHoursErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            QuietHoursErrors::BornsTooHigh => {
                "quiet-hours bound is not a minute of the day (expected < 1440)"
            }
            QuietHoursErrors::StartInferiorToEnd => {
                "quiet-hours window must start after it ends (it wraps midnight)"
            }
            QuietHoursErrors::EmptyWindow => {
                "quiet-hours window is empty (start and end are the same minute)"
            }
        };
        f.write_str(msg)
    }
}

impl std::error::Error for QuietHoursErrors {}

// ============================== Quiet Hours implementation ==============================

/// The nightly window during which the bot stops buying tickets; both bounds
/// are minutes since midnight, Europe/Paris.
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub struct QuietHours {
    /// Start of the quiet hours session
    /// Is the number of minutes since midnight
    /// ex: 23h00 = 1380
    pub start: u16,
    /// End of the quiet hours session
    /// Is the number of minutes since midnight
    /// ex: 8h00 = 480
    pub end: u16,
}

impl QuietHours {
    /// QuietHours is composed of a start and an end hours
    /// expressed in minutes since midnight
    ///
    /// # Arguments
    /// * `start` - Beginning hours of the quiethours window
    /// * `end` - End hours of the quiethours window
    ///
    /// # Returns
    /// Either Self or a QuietHoursError
    pub fn new(start: u16, end: u16) -> Result<Self, QuietHoursErrors> {
        // A minute of the day is 0..1439: 1440 is midnight of the next day,
        // which is spelled `0`.
        if start >= MAX_MINUTES_FROM_MIDNIGHT || end >= MAX_MINUTES_FROM_MIDNIGHT {
            return Err(QuietHoursErrors::BornsTooHigh);
        }

        // Same minute on both sides: the window has no length at all.
        if start == end {
            return Err(QuietHoursErrors::EmptyWindow);
        }

        // The quiet window is expected to be the night: it wraps midnight, so
        // its start comes after its end on the clock.
        if start < end {
            return Err(QuietHoursErrors::StartInferiorToEnd);
        }

        Ok(QuietHours { start, end })
    }

    /// Reads the quiet window from the environment: `QUIET_HOURS_START` and
    /// `QUIET_HOURS_END`, in minutes since midnight (Europe/Paris).
    ///
    /// An absent variable falls back to [`DEFAULT`] for its own side; a bound
    /// that is present but unparsable makes the **whole pair** fall back to
    /// [`DEFAULT`] (one warning, never a half-applied window).
    ///
    /// # Returns
    /// The resolved window — always a valid one.
    pub fn from_env() -> Self {
        Self::from_parts(
            std::env::var("QUIET_HOURS_START").ok().as_deref(),
            std::env::var("QUIET_HOURS_END").ok().as_deref(),
        )
    }

    /// [`QuietHours::from_env`] without the environment access: `None` means
    /// the variable is absent, `Some` its raw (untrimmed) value.
    ///
    /// The clock-free form is what the tests drive, so no test ever touches
    /// the process environment.
    ///
    /// # Arguments
    /// * `start` - raw `QUIET_HOURS_START` value, or `None` when unset
    /// * `end` - raw `QUIET_HOURS_END` value, or `None` when unset
    ///
    /// # Returns
    /// The resolved window — always a valid one, [`DEFAULT`] on any error.
    fn from_parts(start: Option<&str>, end: Option<&str>) -> Self {
        // An invalid side poisons the whole pair: a typo must not pair a
        // hand-set start with the default end.
        let parse = |raw: Option<&str>, default: u16| match raw {
            Some(raw) => raw.trim().parse::<u16>().ok(),
            None => Some(default),
        };
        let (start, end) = match (parse(start, DEFAULT.start), parse(end, DEFAULT.end)) {
            (Some(start), Some(end)) => (start, end),
            _ => {
                tracing::warn!("invalid quiet-hours bound in the environment; using 23:00–08:00");
                return DEFAULT;
            }
        };

        match Self::new(start, end) {
            Ok(quiet) => quiet,
            Err(e) => {
                tracing::warn!("invalid quiet-hours config ({e}); using 23:00–08:00");
                DEFAULT
            }
        }
    }

    /// Whether `minute` falls in the quiet window, which is half-open
    /// `[start, end)` and wraps midnight when `start > end`.
    ///
    /// # Arguments
    /// * `minute` - wall-clock minute of the day (0..1440, Europe/Paris)
    ///
    /// # Returns
    /// `true` when the minute is inside the window
    pub fn contains(&self, minute: u16) -> bool {
        if self.start < self.end {
            (self.start..self.end).contains(&minute)
        } else {
            // The window wraps midnight: it spans the end of day and the
            // beginning of the next one.
            minute >= self.start || minute < self.end
        }
    }

    /// Whether the quiet window is active right now (Europe/Paris wall clock).
    ///
    /// # Returns
    /// `true` when the bot is inside the quiet window
    pub fn is_quiet_hours(&self) -> bool {
        self.contains(paris_minutes_now())
    }

    /// [`QuietHours::is_quiet_hours`] at a fixed instant — the clock-free form
    /// used by tests, so DST cases do not depend on when the suite runs.
    ///
    /// # Arguments
    /// * `now` - the instant to test
    ///
    /// # Returns
    /// `true` when `now` falls inside the quiet window
    pub fn is_quiet_at(&self, now: Timestamp) -> bool {
        self.contains(paris_minutes_at(now))
    }
}

/// Renders the window as `HH:MM–HH:MM` (Europe/Paris), for log lines.
impl std::fmt::Display for QuietHours {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:0>2}:{:0>2}–{:0>2}:{:0>2}",
            self.start / 60,
            self.start % 60,
            self.end / 60,
            self.end % 60
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Builds a fixed instant from an RFC 3339 fixture (never reads the clock).
    fn at(rfc3339: &str) -> Timestamp {
        rfc3339.parse().expect("valid timestamp fixture")
    }

    // Builds a valid window, failing loudly on a fixture mistake.
    fn window(start: u16, end: u16) -> QuietHours {
        QuietHours::new(start, end).expect("fixture window must be valid")
    }

    // The shipped window: 23:00 -> 08:00 Paris (it wraps midnight).
    fn night() -> QuietHours {
        window(DEFAULT.start, DEFAULT.end)
    }

    // ============================== DEFAULT ==============================

    #[test]
    fn default_is_the_documented_night_window() {
        assert_eq!(DEFAULT.start, 23 * 60);
        assert_eq!(DEFAULT.end, 8 * 60);
    }

    #[test]
    fn default_is_a_valid_window() {
        assert_eq!(QuietHours::new(DEFAULT.start, DEFAULT.end), Ok(DEFAULT));
    }

    // ============================== QuietHours::new ==============================

    #[test]
    fn new_rejects_bounds_outside_the_day() {
        // 1440 is midnight of the next day: the last minute of a day is 1439.
        assert_eq!(
            QuietHours::new(1440, 0),
            Err(QuietHoursErrors::BornsTooHigh)
        );
        assert_eq!(
            QuietHours::new(0, 1440),
            Err(QuietHoursErrors::BornsTooHigh)
        );
        assert_eq!(
            QuietHours::new(u16::MAX, 0),
            Err(QuietHoursErrors::BornsTooHigh)
        );
        assert_eq!(
            QuietHours::new(1441, 1441),
            Err(QuietHoursErrors::BornsTooHigh)
        );
        assert_eq!(
            QuietHours::new(1439, 1441),
            Err(QuietHoursErrors::BornsTooHigh)
        );
        assert_eq!(
            QuietHours::new(1441, 1439),
            Err(QuietHoursErrors::BornsTooHigh)
        );
    }

    #[test]
    fn new_rejects_an_empty_window() {
        // Equal bounds would otherwise read as "quiet all day".
        assert_eq!(
            QuietHours::new(1380, 1380),
            Err(QuietHoursErrors::EmptyWindow)
        );
        assert_eq!(QuietHours::new(0, 0), Err(QuietHoursErrors::EmptyWindow));
    }

    #[test]
    fn new_rejects_a_non_wrapping_window() {
        assert_eq!(
            QuietHours::new(480, 1380),
            Err(QuietHoursErrors::StartInferiorToEnd)
        );
        assert_eq!(
            QuietHours::new(500, 1000),
            Err(QuietHoursErrors::StartInferiorToEnd)
        );
    }

    #[test]
    fn new_accepts_the_wrapping_window() {
        assert_eq!(
            window(1380, 480),
            QuietHours {
                start: 1380,
                end: 480
            }
        );
        // The shortest expressible windows: one minute long.
        assert_eq!(
            window(1439, 0),
            QuietHours {
                start: 1439,
                end: 0
            }
        );
        assert_eq!(window(1, 0), QuietHours { start: 1, end: 0 });
    }

    // ============================== Errors ==============================

    #[test]
    fn display_explains_a_bound_past_the_day() {
        assert_eq!(
            QuietHoursErrors::BornsTooHigh.to_string(),
            "quiet-hours bound is not a minute of the day (expected < 1440)"
        );
    }

    #[test]
    fn display_explains_a_non_wrapping_window() {
        assert_eq!(
            QuietHoursErrors::StartInferiorToEnd.to_string(),
            "quiet-hours window must start after it ends (it wraps midnight)"
        );
    }

    #[test]
    fn display_explains_an_empty_window() {
        assert_eq!(
            QuietHoursErrors::EmptyWindow.to_string(),
            "quiet-hours window is empty (start and end are the same minute)"
        );
    }

    #[test]
    fn errors_expose_no_source() {
        assert!(std::error::Error::source(&QuietHoursErrors::BornsTooHigh).is_none());
        assert!(std::error::Error::source(&QuietHoursErrors::StartInferiorToEnd).is_none());
        assert!(std::error::Error::source(&QuietHoursErrors::EmptyWindow).is_none());
    }

    // ============================== QuietHours::contains ==============================

    // The window is half-open: start is in, end is out, on both sides of
    // midnight.
    #[test]
    fn contains_is_half_open_at_both_edges() {
        let night = night();
        assert!(!night.contains(1379)); // 22:59, one minute before the start
        assert!(night.contains(1380)); // 23:00, the start itself
        assert!(night.contains(1439)); // 23:59
        assert!(night.contains(0)); // 00:00
        assert!(night.contains(1)); // 00:01
        assert!(night.contains(479)); // 07:59, the last quiet minute
        assert!(!night.contains(480)); // 08:00, the end itself
        assert!(!night.contains(481)); // 08:01
    }

    // `new` refuses a day window, but the branch is still reachable through
    // the public fields and must stay consistent.
    #[test]
    fn contains_supports_a_day_window() {
        let day = QuietHours {
            start: 480,
            end: 1380,
        };
        assert!(!day.contains(479));
        assert!(day.contains(480));
        assert!(day.contains(1379));
        assert!(!day.contains(1380));
    }

    // Exhaustive oracle: the window is `[start, end)` modulo a day, so the
    // offset from the start must be shorter than the window length. Catches
    // every off-by-one at the edges, on wrapping and non-wrapping windows.
    #[test]
    fn contains_agrees_with_modular_arithmetic_for_every_minute() {
        let windows = [
            window(1380, 480),
            window(1439, 0),
            window(1, 0),
            QuietHours {
                start: 480,
                end: 1380,
            },
        ];
        for w in windows {
            let length = (u32::from(w.end) + 1440 - u32::from(w.start)) % 1440;
            for minute in 0..1440u16 {
                let offset = (u32::from(minute) + 1440 - u32::from(w.start)) % 1440;
                assert_eq!(
                    w.contains(minute),
                    offset < length,
                    "window {w:?}, minute {minute}"
                );
            }
        }
    }

    // ============================== QuietHours::is_quiet_at ==============================

    // The shipped window, checked on the Paris wall clock (CET, +01:00).
    #[test]
    fn is_quiet_at_reads_the_paris_night() {
        let night = night();
        assert!(!night.is_quiet_at(at("2026-01-15T21:59:00Z"))); // 22:59
        assert!(night.is_quiet_at(at("2026-01-15T22:00:00Z"))); // 23:00
        assert!(night.is_quiet_at(at("2026-01-16T06:59:00Z"))); // 07:59
        assert!(!night.is_quiet_at(at("2026-01-16T07:00:00Z"))); // 08:00
    }

    // The verdict is read in Paris, never in UTC: in summer the offset is
    // +02:00 and both assertions flip if the conversion is wrong.
    #[test]
    fn is_quiet_at_reads_paris_not_utc() {
        let night = night();
        // 21:30Z = 23:30 Paris (CEST): inside, while UTC says 21:30.
        assert!(night.is_quiet_at(at("2026-07-15T21:30:00Z")));
        // 06:30Z = 08:30 Paris (CEST): outside, while UTC (06:30) is inside.
        assert!(!night.is_quiet_at(at("2026-07-16T06:30:00Z")));
    }

    // Spring forward: 02:00-03:00 does not exist, the local clock jumps from
    // 01:59 to 03:00 — the quiet window must follow the wall clock, not UTC.
    #[test]
    fn is_quiet_at_handles_the_spring_forward_gap() {
        let night = window(1380, 180); // 23:00 -> 03:00
        assert!(!night.is_quiet_at(at("2026-03-28T21:59:00Z"))); // 22:59 CET
        assert!(night.is_quiet_at(at("2026-03-28T23:30:00Z"))); // 00:30 CET
        assert!(night.is_quiet_at(at("2026-03-29T00:30:00Z"))); // 01:30 CET
        assert!(!night.is_quiet_at(at("2026-03-29T01:00:00Z"))); // 03:00 CEST
        assert!(!night.is_quiet_at(at("2026-03-29T01:30:00Z"))); // 03:30 CEST
    }

    // Autumn overlap: 02:30 happens twice, and both instants are inside.
    #[test]
    fn is_quiet_at_handles_the_autumn_overlap() {
        let night = window(1380, 180); // 23:00 -> 03:00
        assert!(!night.is_quiet_at(at("2026-10-24T19:30:00Z"))); // 21:30 CEST
        assert!(night.is_quiet_at(at("2026-10-24T23:30:00Z"))); // 01:30 CEST
        assert!(night.is_quiet_at(at("2026-10-25T00:30:00Z"))); // 02:30 CEST
        assert!(night.is_quiet_at(at("2026-10-25T01:30:00Z"))); // 02:30 CET
        assert!(!night.is_quiet_at(at("2026-10-25T02:00:00Z"))); // 03:00 CET
    }

    // Midnight is minute 0 on the Paris clock, not the end of the previous day.
    #[test]
    fn is_quiet_at_at_the_midnight_boundary() {
        assert!(night().is_quiet_at(at("2026-01-15T23:00:00Z"))); // 00:00 Paris
    }

    // The public entry point agrees with the pure check on the same instant
    // (wiring test; the window policy is covered above).
    #[test]
    fn is_quiet_hours_agrees_with_is_quiet_at() {
        let now = Timestamp::now();
        assert_eq!(night().is_quiet_hours(), night().is_quiet_at(now));
    }

    // ============================== QuietHours::from_parts ==============================

    #[test]
    fn parts_fall_back_to_default_when_absent() {
        assert_eq!(QuietHours::from_parts(None, None), DEFAULT);
    }

    #[test]
    fn parts_trim_surrounding_whitespace() {
        assert_eq!(
            QuietHours::from_parts(Some(" 1380 "), Some("\t480\n")),
            DEFAULT
        );
    }

    #[test]
    fn parts_accept_a_custom_window() {
        assert_eq!(
            QuietHours::from_parts(Some("1320"), Some("360")),
            QuietHours {
                start: 1320,
                end: 360
            }
        );
    }

    #[test]
    fn parts_ignore_a_garbage_value() {
        assert_eq!(QuietHours::from_parts(Some("abc"), Some("480")), DEFAULT);
        assert_eq!(QuietHours::from_parts(Some(""), Some("480")), DEFAULT);
    }

    #[test]
    fn parts_ignore_an_overflowing_value() {
        // `u16::parse` rejects it before the day-range check can see it.
        assert_eq!(QuietHours::from_parts(Some("70000"), Some("480")), DEFAULT);
    }

    #[test]
    fn parts_ignore_a_negative_value() {
        assert_eq!(QuietHours::from_parts(Some("-60"), Some("480")), DEFAULT);
    }

    #[test]
    fn parts_fall_back_when_the_pair_is_not_wrapping() {
        assert_eq!(QuietHours::from_parts(Some("480"), Some("1380")), DEFAULT);
    }

    // A single bad bound poisons the pair: no half-applied window.
    #[test]
    fn parts_fall_back_when_one_bound_is_invalid() {
        assert_eq!(QuietHours::from_parts(Some("abc"), Some("360")), DEFAULT);
        assert_eq!(QuietHours::from_parts(Some("1320"), Some("abc")), DEFAULT);
    }

    // An absent side takes its own default; the other one is kept.
    #[test]
    fn parts_default_the_missing_side_only() {
        assert_eq!(
            QuietHours::from_parts(Some("1200"), None),
            QuietHours {
                start: 1200,
                end: DEFAULT.end
            }
        );
        assert_eq!(
            QuietHours::from_parts(None, Some("360")),
            QuietHours {
                start: DEFAULT.start,
                end: 360
            }
        );
    }

    // ============================== phase_change ==============================

    // The startup tick carries no previous state: it never reports a
    // transition, whatever the current phase is.
    #[test]
    fn phase_change_is_quiet_at_startup() {
        assert_eq!(phase_change(None, true), None);
        assert_eq!(phase_change(None, false), None);
    }

    #[test]
    fn phase_change_reports_the_entry_and_the_exit() {
        assert_eq!(phase_change(Some(false), true), Some(QuietChange::Started));
        assert_eq!(phase_change(Some(true), false), Some(QuietChange::Ended));
    }

    // Two ticks in the same phase are not a transition, in either direction.
    #[test]
    fn phase_change_is_silent_without_a_phase_switch() {
        assert_eq!(phase_change(Some(false), false), None);
        assert_eq!(phase_change(Some(true), true), None);
    }

    // ============================== Display ==============================

    // Zero-padded HH:MM on both bounds, joined by the en dash used in logs.
    #[test]
    fn display_renders_the_window_as_clock_times() {
        assert_eq!(night().to_string(), "23:00–08:00");
        assert_eq!(window(1380, 480).to_string(), "23:00–08:00");
        // A day window is not constructible via `new`, but Display must
        // still render whatever the fields hold.
        assert_eq!(
            QuietHours {
                start: 480,
                end: 1380
            }
            .to_string(),
            "08:00–23:00"
        );
    }
}
