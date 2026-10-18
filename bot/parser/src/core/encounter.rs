use crate::core::club::ClubType;
/// This module defines the Encounter struct, which represents a sports match or event,
/// including its title, date, nature, and associated seats.
/// It also includes the MatchNature enum to categorize the type of match (e.g., rugby, basketball).
use crate::core::seat::Seat;
use chrono::{Datelike, NaiveDate, NaiveDateTime, Utc};
use chrono_tz::Europe::Paris;
use std::sync::atomic::{AtomicU64, Ordering};
/// MatchNature is an enumeration that categorizes the type of match or event, such as rugby, basketball, or other.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum MatchNature {
    Rugby,
    Basketball,
    Other,
}

/// Implementation of the MatchNature enum, including a method to determine the match nature from a title string.
impl MatchNature {
    /// Determines the MatchNature based on the content of the title string.
    ///
    /// # Arguments
    /// * `title` - The title of the match or event as a string
    /// # Returns
    /// The corresponding MatchNature enum variant based on the title content
    pub fn from_title(title: &str) -> Self {
        let lower = title.to_lowercase();
        if lower.contains("basket") {
            MatchNature::Basketball
        } else if lower.starts_with("stade rochelais") {
            MatchNature::Rugby
        } else {
            MatchNature::Other
        }
    }

    /// Determines the MatchNature based on the badge text of a match card.
    ///
    /// The ticketing site now shows the sport as a badge (e.g. "RUGBY", "BASKET")
    /// while the card title only contains the opponent name.
    ///
    /// # Arguments
    /// * `badge` - The badge text of the match card
    /// # Returns
    /// The corresponding MatchNature enum variant based on the badge content
    pub fn from_badge(badge: &str) -> Self {
        let upper = badge.to_uppercase();
        if upper.contains("RUGBY") {
            MatchNature::Rugby
        } else if upper.contains("BASKET") {
            MatchNature::Basketball
        } else {
            MatchNature::Other
        }
    }
}

/// Encounter represents a sports match or event, including its title, date, nature, and associated seats.
///
/// It includes an ID for database storage, the club type, and an optional resale link for tickets.
///
/// The seats field is optional and can be set after the encounter is created,
/// allowing for a two-step parsing process where encounters are first identified and then detailed seat
/// information is added later.
///
#[derive(Debug, Clone)]
pub struct Encounter {
    pub id: u64,
    pub club_type: ClubType,
    pub title: String,
    pub date: String,
    pub nature: MatchNature,
    pub resale_link: Option<String>,
    pub seats: Option<Vec<Seat>>,
}

/// A static atomic counter to generate unique IDs for encounters when they are created.
static ENCOUNTER_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Implementation of the Encounter struct, including a constructor and a method to set the seats information.
impl Encounter {
    /// Creates a new Encounter instance with the given club type, title, date, nature, and optional resale link.
    ///
    /// # Arguments
    /// * `club_type` - The type of the club associated with the encounter (from the ClubType enum)
    /// * `title` - The title of the encounter as a string
    /// * `date` - The date of the encounter as a string in natural language format (e.g., "Samedi 16 septembre 2023 à 14:00")
    /// * `nature` - The nature of the encounter (from the MatchNature enum)
    /// * `resale_link` - An optional resale link for the encounter's tickets
    /// # Returns
    /// A new instance of the Encounter struct with the provided information and a unique ID
    pub fn new(
        club_type: ClubType,
        title: String,
        date: String,
        nature: MatchNature,
        resale_link: Option<String>,
    ) -> Self {
        Self {
            id: ENCOUNTER_COUNTER.fetch_add(1, Ordering::Relaxed),
            club_type,
            title,
            date,
            nature,
            resale_link,
            seats: None,
        }
    }

    /// Sets the seats information for the encounter.
    ///
    /// # Arguments
    /// * `seats` - A vector of Seat instances representing the seats available for the encounter
    /// # Returns
    /// This method does not return a value, but it updates the seats field of the Encounter instance with the provided seats information
    pub fn set_seats(&mut self, seats: Vec<Seat>) {
        self.seats = Some(seats);
    }

    /// Map of French month name to month number (1-based).
    /// Try to parse a French date string into a `NaiveDateTime`.
    ///
    /// Handles these formats:
    /// - `"samedi 6 juin 2025 à 21h05"`      → full date with time
    /// - `"dimanche 17 mai à 21h05"`          → missing year (uses current year)
    /// - `"week-end du 06/07 mai"`            → day range, uses the first day
    ///
    /// Returns `None` when the string cannot be parsed.
    fn try_parse_date(&self) -> Option<NaiveDateTime> {
        parse_french_date(&self.date)
    }

    /// Check if the encounter date has already passed (i.e. the match is over).
    ///
    /// Returns `true` if the encounter date has already passed (i.e. the match is over).
    pub fn date_passed(&self) -> bool {
        self.try_parse_date()
            .map(|dt| dt < Utc::now().with_timezone(&Paris).naive_local())
            .unwrap_or(false)
    }

    /// Returns the date as a formatted string suitable for sorting: `"2025-06-15 21:05"`.
    /// Falls back to the original raw string if parsing fails.
    pub fn formatted_date(&self) -> String {
        self.try_parse_date()
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| self.date.clone())
    }
}

/// Parse a French natural-language date string into a `NaiveDateTime`.
///
/// Supported input examples:
/// - `"samedi 6 juin à 21h05"`
/// - `"samedi 6 juin 2025 à 21h05"`
/// - `"week-end du 06/07 mai"`
/// - `"week-end du 06/07 mai 2025"`
///
/// When the year is missing, the current year is used.
///
/// # Arguments
/// * `s` - A string slice containing the French date string to be parsed
///
/// # Returns
/// An `Option<NaiveDateTime>` containing the parsed date and time, or `None` if the string cannot be parsed
fn parse_french_date(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Map French month names to numbers (1-based)
    let months: &[(&str, u32)] = &[
        ("janvier", 1),
        ("février", 2),
        ("mars", 3),
        ("avril", 4),
        ("mai", 5),
        ("juin", 6),
        ("juillet", 7),
        ("août", 8),
        ("septembre", 9),
        ("octobre", 10),
        ("novembre", 11),
        ("décembre", 12),
    ];

    // 1) Normalize time: "21h05" → "21:05"
    let s = s.replace('h', ":");

    // 2) Extract day + month from either "week-end du 06/07 month" or "week-day DD month"
    let words: Vec<&str> = s.split_whitespace().collect();

    // Find the month name position and get the day number from the preceding word
    let mut day: Option<u32> = None;
    let mut month: Option<u32> = None;
    let mut year: Option<i32> = None;
    let mut time_part: Option<&str> = None;

    for (i, word) in words.iter().enumerate() {
        if let Some(&(_, m)) = months.iter().find(|(name, _)| *name == *word) {
            month = Some(m);
            // Day is the previous word, or the first part of "DD/DD" for week-end
            if i > 0 {
                let prev = words[i - 1];
                if let Some(d) = parse_french_day(prev) {
                    day = Some(d);
                }
            }
        }
        // Check for year (a 4-digit number)
        if word.len() == 4
            && word.chars().all(|c| c.is_ascii_digit())
            && let Ok(y) = word.parse::<i32>()
            && (2020..=2100).contains(&y)
        {
            year = Some(y);
        }
        // Check for time (HH:MM pattern)
        if word.contains(':') && word.len() <= 5 {
            let parts: Vec<&str> = word.split(':').collect();
            if parts.len() == 2
                && parts[0].len() <= 2
                && parts[1].len() == 2
                && parts[0].chars().all(|c| c.is_ascii_digit())
                && parts[1].chars().all(|c| c.is_ascii_digit())
            {
                time_part = Some(word);
            }
        }
    }

    let month = month?;
    let day = day?;
    let year = year.unwrap_or_else(|| {
        let now = Utc::now().naive_utc();
        now.year()
    });

    // Build the date string
    if let Some(time) = time_part {
        NaiveDateTime::parse_from_str(
            &format!("{:04}-{:02}-{:02} {}", year, month, day, time),
            "%Y-%m-%d %H:%M",
        )
        .ok()
    } else {
        NaiveDate::from_ymd_opt(year, month, day).map(|d| d.and_hms_opt(0, 0, 0).unwrap())
    }
}

/// Parser for a French day string, which may be a simple number or a range (e.g., "06/07").
///
/// # Arguments
/// * `word` - A string slice representing the day or day range in French format
///
/// # Returns
/// An `Option<u32>` containing the parsed day number, or `None` if the string cannot be parsed.
///
/// Extract a day number from a word that may be:
/// - A simple number: `"6"` → `6`
/// - A day range: `"06/07"` → `6` (first day)
fn parse_french_day(word: &str) -> Option<u32> {
    // "week-end du 06/07" → take "06"
    if word.contains('/') {
        let last = word.split('/').nth(1)?;
        return last.parse::<u32>().ok();
    }
    word.parse::<u32>().ok()
}

/// Command to launch these tests :  cargo test -p parser --lib encounter -- --nocapture
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::club::ClubType;
    use chrono::Timelike;

    fn enc(date: &str) -> Encounter {
        Encounter::new(
            ClubType::StadeRochelais,
            "Test".to_string(),
            date.to_string(),
            MatchNature::Rugby,
            None,
        )
    }

    #[test]
    fn parse_weekday_day_month_time() {
        let e = enc("samedi 6 juin à 21h05");
        let dt = e.try_parse_date();
        assert!(dt.is_some(), "Failed to parse: {}", e.date);
        let dt = dt.unwrap();
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.day(), 6);
        assert_eq!(dt.hour(), 21);
        assert_eq!(dt.minute(), 5);
    }

    #[test]
    fn parse_weekday_day_month_time_without_h() {
        let e = enc("dimanche 17 mai à 21:05");
        let dt = e.try_parse_date();
        assert!(dt.is_some(), "Failed to parse: {}", e.date);
        let dt = dt.unwrap();
        assert_eq!(dt.month(), 5);
        assert_eq!(dt.day(), 17);
        assert_eq!(dt.hour(), 21);
    }

    #[test]
    fn parse_weekend_range_uses_last_day() {
        let e = enc("week-end du 06/07 juin");
        let dt = e.try_parse_date();
        assert!(dt.is_some(), "Failed to parse: {}", e.date);
        let dt = dt.unwrap();
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.day(), 7);
    }

    #[test]
    fn parse_weekend_range_second() {
        let e = enc("week-end du 16/17 mai");
        let dt = e.try_parse_date();
        assert!(dt.is_some(), "Failed to parse: {}", e.date);
        let dt = dt.unwrap();
        assert_eq!(dt.month(), 5);
        assert_eq!(dt.day(), 17);
    }

    #[test]
    fn parse_empty_returns_none() {
        let e = enc("");
        assert!(e.try_parse_date().is_none());
    }

    #[test]
    fn parse_garbage_returns_none() {
        let e = enc("not a date at all");
        assert!(e.try_parse_date().is_none());
    }

    #[test]
    fn formatted_date_output() {
        let e = enc("samedi 6 juin à 21h05");
        let formatted = e.formatted_date();
        // Should contain the date in YYYY-MM-DD HH:MM format
        assert!(
            formatted.contains("06-06"),
            "Expected month-day in formatted date, got: {}",
            formatted
        );
        assert!(
            formatted.contains("21:05"),
            "Expected time in formatted date, got: {}",
            formatted
        );
    }

    #[test]
    fn date_passed_returns_true_for_past_date() {
        let e = enc("samedi 6 juin 2020 à 21h05");
        assert!(e.date_passed(), "2020 date should be passed");
    }

    #[test]
    fn date_passed_returns_false_for_future_date() {
        let e = enc("samedi 6 juin 2099 à 21h05");
        assert!(!e.date_passed(), "2099 date should NOT be passed");
    }

    #[test]
    fn date_passed_returns_false_for_weekend_future() {
        let e = enc("week-end du 06/07 juin 2099");
        assert!(!e.date_passed(), "2099 weekend should NOT be passed");
    }

    #[test]
    fn date_passed_returns_false_when_unparseable() {
        let e = enc("");
        assert!(
            !e.date_passed(),
            "Empty date should not be considered passed"
        );
    }

    #[test]
    fn parse_basketball_date() {
        let e = enc("jeudi 30 avril à 20h00");
        let dt = e.try_parse_date();
        assert!(dt.is_some(), "Failed to parse: {}", e.date);
        let dt = dt.unwrap();
        assert_eq!(dt.month(), 4);
        assert_eq!(dt.day(), 30);
        assert_eq!(dt.hour(), 20);
    }

    #[test]
    fn parse_with_explicit_year() {
        let e = enc("samedi 6 juin 2025 à 21h05");
        let dt = e.try_parse_date();
        assert!(dt.is_some(), "Failed to parse: {}", e.date);
        let dt = dt.unwrap();
        assert_eq!(dt.year(), 2025);
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.day(), 6);
    }
}
