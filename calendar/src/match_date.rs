//! A precise kickoff day+time, without year or timestamp.
//!
//! The site displays kickoffs in French ("samedi 10 octobre", "14h30") with an
//! implicit Europe/Paris timezone. This module only checks whether a kickoff is
//! precise enough to consider a match `Scheduled`; it does not build timestamps.

//! A precise kickoff day+time, without year or timestamp.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchDate {
    pub day: u8,
    pub month: u8,
    pub hour: u8,
    pub minute: u8,
}

const FRENCH_MONTHS: [&str; 12] = [
    "janvier",
    "fevrier",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "aout",
    "septembre",
    "octobre",
    "novembre",
    "decembre",
];

impl MatchDate {
    /// Returns `None` when the date/time is not precise (i.e. the match is TBD).
    pub fn parse(date: &str, time: &str) -> Option<MatchDate> {
        let time = time.trim();
        if time.is_empty() || time == "-" {
            return None;
        }

        let date = date.trim();
        if date.contains('–') || date.contains('—') || date.contains(" à ") {
            return None;
        }

        let tokens: Vec<&str> = date.split_whitespace().collect();
        let day = tokens.iter().find_map(|token| token.parse::<u8>().ok())?;
        let month = tokens.iter().find_map(|token| month_number(token))?;

        let (hour, minute) = time.split_once('h')?;
        let hour = hour.trim().parse::<u8>().ok()?;
        let minute = minute.trim().parse::<u8>().ok()?;

        Some(MatchDate {
            day,
            month,
            hour,
            minute,
        })
    }
}

/// The month index (1..=12) of a French month token ("octobre", "féVRier"),
/// after accent normalization.
///
/// # Returns
/// `Some(1..=12)`, or `None` for a non-month token.
fn month_number(token: &str) -> Option<u8> {
    let normalized = normalize(token);
    FRENCH_MONTHS
        .iter()
        .position(|month| *month == normalized)
        .map(|index| (index + 1) as u8)
}

/// An inclusive window of days of the year, expressed as `(month, day)` pairs
/// without the year. Windows never span a year boundary in one LNR season, in
/// which every month occurs exactly once, so plain tuple ordering is correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateWindow {
    pub from: (u8, u8),
    pub to: (u8, u8),
}

impl DateWindow {
    /// Returns `true` when both windows share at least one common day.
    pub fn overlaps(&self, other: &DateWindow) -> bool {
        !(self.to < other.from || other.to < self.from)
    }
}

/// Extracts an inclusive `(from, to)` date window from a French date string,
/// tolerating every format both sides use: LNR kickoffs ("samedi 10 octobre
/// 14h30", "samedi 28 novembre – dimanche 29 novembre") and billetterie
/// display dates ("dimanche 25 octobre à 21h05", "week-end du 28/29
/// novembre", "week-end du 02/03 janvier").
///
/// Year-less by construction: within one season each month occurs exactly
/// once, so a `(month, day)` window is unambiguous — no year inference is
/// needed. Returns `None` when no recognizable month or day is present.
pub fn parse_window(date: &str) -> Option<DateWindow> {
    let normalized = normalize(date);
    let tokens: Vec<&str> = normalized.split_whitespace().collect();

    let month = tokens.iter().find_map(|token| month_number(token))?;

    let mut days: Vec<u8> = Vec::new();
    for token in tokens {
        if let Ok(day) = token.parse::<u8>() {
            if (1..=31).contains(&day) {
                days.push(day);
            }
        } else if let Some((a, b)) = token.split_once('/') {
            if let (Ok(a), Ok(b)) = (a.parse::<u8>(), b.parse::<u8>()) {
                if (1..=31).contains(&a) && (1..=31).contains(&b) {
                    days.push(a);
                    days.push(b);
                }
            }
        }
    }

    let from_day = *days.iter().min()?;
    let to_day = *days.iter().max()?;
    Some(DateWindow {
        from: (month, from_day),
        to: (month, to_day),
    })
}

/// Lower-cases the input and drops French accents, so month/day matching
/// works whatever accents the page uses ("février" == "fevrier").
fn normalize(input: &str) -> String {
    input
        .to_lowercase()
        .replace(['é', 'è', 'ê', 'ë'], "e")
        .replace(['à', 'â'], "a")
        .replace(['û', 'ù'], "u")
        .replace('ô', "o")
        .replace(['î', 'ï'], "i")
        .replace('ç', "c")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses a precise French kickoff ("samedi 10 octobre", "14h30") into a
    /// `MatchDate` with the expected day, month, hour and minute.
    #[test]
    fn parses_precise_date_and_time() {
        let date = MatchDate::parse("samedi 10 octobre", "14h30").unwrap();
        assert_eq!(date.day, 10);
        assert_eq!(date.month, 10);
        assert_eq!(date.hour, 14);
        assert_eq!(date.minute, 30);
    }

    /// Returns `None` when the time is missing or a dash, meaning the match is
    /// still TBD and not precise enough to be `Scheduled`.
    #[test]
    fn returns_none_for_tbd_time() {
        assert!(MatchDate::parse("samedi 10 octobre", "-").is_none());
        assert!(MatchDate::parse("samedi 10 octobre", "").is_none());
    }

    /// Returns `None` for a multi-day range, which is not a single precise kickoff.
    #[test]
    fn returns_none_for_date_range() {
        assert!(MatchDate::parse("samedi 28 novembre – dimanche 29 novembre", "14h30").is_none());
    }

    /// Returns `None` when the month name is not recognized (e.g. a typo).
    #[test]
    fn returns_none_for_unknown_month() {
        assert!(MatchDate::parse("samedi 10 octombre", "14h30").is_none());
    }

    /// Parses accented month names (e.g. "février") by normalizing accents.
    #[test]
    fn parses_accented_months() {
        let date = MatchDate::parse("vendredi 5 février", "20h05").unwrap();
        assert_eq!(date.month, 2);
        assert_eq!(date.day, 5);
        assert_eq!(date.hour, 20);
        assert_eq!(date.minute, 5);
    }

    /// `parse_window` turns a precise kickoff into a single-point window.
    #[test]
    fn parse_window_point_date() {
        assert_eq!(
            parse_window("samedi 10 octobre 14h30"),
            Some(DateWindow {
                from: (10, 10),
                to: (10, 10)
            })
        );
    }

    /// `parse_window` tolerates the LNR multi-day range format.
    #[test]
    fn parse_window_dash_range() {
        assert_eq!(
            parse_window("samedi 28 novembre – dimanche 29 novembre"),
            Some(DateWindow {
                from: (11, 28),
                to: (11, 29)
            })
        );
    }

    /// `parse_window` tolerates the " à " range format (day spans, not times).
    #[test]
    fn parse_window_a_range() {
        assert_eq!(
            parse_window("samedi 10 à 11 octobre"),
            Some(DateWindow {
                from: (10, 10),
                to: (10, 11)
            })
        );
    }

    /// `parse_window` tolerates the billetterie "week-end du DD/DD month" format.
    #[test]
    fn parse_window_slash_range() {
        assert_eq!(
            parse_window("week-end du 28/29 novembre"),
            Some(DateWindow {
                from: (11, 28),
                to: (11, 29)
            })
        );
        assert_eq!(
            parse_window("week-end du 02/03 janvier"),
            Some(DateWindow {
                from: (1, 2),
                to: (1, 3)
            })
        );
    }

    /// `parse_window` ignores times ("21h05", "14h30") as day candidates.
    #[test]
    fn parse_window_ignores_times() {
        assert_eq!(
            parse_window("dimanche 25 octobre à 21h05"),
            Some(DateWindow {
                from: (10, 25),
                to: (10, 25)
            })
        );
        assert_eq!(
            parse_window("samedi 10 octobre 14h30 21h05"),
            Some(DateWindow {
                from: (10, 10),
                to: (10, 10)
            })
        );
    }

    /// `parse_window` returns `None` for empty or dayless date strings.
    #[test]
    fn parse_window_rejects_empty_and_dayless() {
        assert_eq!(parse_window(""), None);
        assert_eq!(parse_window("samedi octobre"), None);
        assert_eq!(parse_window("samedi 40 octombre"), None);
    }

    /// `overlaps` compares windows on the shared `(month, day)` space.
    #[test]
    fn window_overlap() {
        let same_day = DateWindow {
            from: (10, 25),
            to: (10, 25),
        };
        assert!(same_day.overlaps(&same_day));

        // A billetterie two-day window covers a kickoff on the second day.
        let weekend = DateWindow {
            from: (11, 28),
            to: (11, 29),
        };
        let kickoff = DateWindow {
            from: (11, 29),
            to: (11, 29),
        };
        assert!(weekend.overlaps(&kickoff));
        assert!(!same_day.overlaps(&weekend));

        // Different months never overlap.
        let december = DateWindow {
            from: (12, 19),
            to: (12, 20),
        };
        assert!(!weekend.overlaps(&december));
    }
}
