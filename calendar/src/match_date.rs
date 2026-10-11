//! A precise kickoff day+time, without year or timestamp.
//!
//! The site displays kickoffs in French ("samedi 10 octobre", "14h30") with an
//! implicit Europe/Paris timezone. This module only checks whether a kickoff is
//! precise enough to consider a match `Scheduled`; it does not build timestamps.

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

fn month_number(token: &str) -> Option<u8> {
    let normalized = normalize(token);
    FRENCH_MONTHS
        .iter()
        .position(|month| *month == normalized)
        .map(|index| (index + 1) as u8)
}

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
}
