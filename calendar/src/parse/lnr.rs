//! top14.lnr.fr parser made to get all the top14 events.

use super::EventParser;
use crate::event::MatchStatus;
use crate::fetcher::WebPageFetcher;
use crate::match_date::MatchDate;
use crate::team::{Team, TeamType};
use crate::{Event, FetchError, ParseError};
use scraper::{ElementRef, Html, Selector};

const LNR_CALENDAR_BASE_URL: &str = "https://top14.lnr.fr/calendrier-et-resultats";
const MAX_JOURNEES: u8 = 50;
const EVENT_SELECTOR: &str = ".calendar-results__line";
const CLUB_NAME_SELECTOR: &str = ".club-line__name";
const TIME_SELECTOR: &str = ".match-line__time";
const DATE_SELECTOR: &str = ".calendar-results__fixture-date";
const SCORE_SELECTOR: &str = ".match-line__score";

pub struct LnrParser {
    season: String,
}

impl LnrParser {
    pub fn new(season: impl Into<String>) -> Self {
        Self {
            season: season.into(),
        }
    }

    fn calendar_base_url(&self) -> String {
        format!("{LNR_CALENDAR_BASE_URL}/{}/j", self.season)
    }
}

impl EventParser for LnrParser {
    fn parse(&self, raw: &str) -> Result<Vec<Event>, ParseError> {
        self.parse_events(raw, 1)
    }

    fn parse_all(&self) -> Result<Vec<Event>, Box<dyn std::error::Error + Send + Sync>> {
        self.fetch_all_events_from(&self.calendar_base_url())
    }
}

impl LnrParser {
    fn parse_events(&self, raw: &str, day_num: u8) -> Result<Vec<Event>, ParseError> {
        // Parse the raw HTML into a traversable document tree.
        let doc = Html::parse_document(raw);

        // Define all the selectors we need to walk the calendar, mapping any
        // invalid selector to a parse error.
        let block_selector = Selector::parse(&format!("{DATE_SELECTOR}, {EVENT_SELECTOR}"))
            .map_err(|e| ParseError::InvalidData(e.to_string()))?;
        let name_selector = Selector::parse(CLUB_NAME_SELECTOR)
            .map_err(|e| ParseError::InvalidData(e.to_string()))?;
        let time_selector =
            Selector::parse(TIME_SELECTOR).map_err(|e| ParseError::InvalidData(e.to_string()))?;
        let score_selector =
            Selector::parse(SCORE_SELECTOR).map_err(|e| ParseError::InvalidData(e.to_string()))?;
        // Strip the leading dot to compare against the raw class names.
        let date_class = DATE_SELECTOR.trim_start_matches('.');

        // Walk date and match blocks in document order, remembering the current
        // date so each match inherits the date that precedes it.
        let mut events = Vec::new();
        let mut current_date = String::new();
        let mut index: usize = 0;
        for block in doc.select(&block_selector) {
            // Date blocks only update the current date; they do not produce events.
            let is_date = block.value().classes().any(|class| class == date_class);
            if is_date {
                current_date = block.text().collect::<String>().trim().to_string();
                continue;
            }

            // Extract the two clubs (home then away); skip rows that do not
            // contain exactly two clubs.
            let clubs: Vec<_> = block.select(&name_selector).collect();
            if clubs.len() != 2 {
                continue;
            }

            // Extract the kickoff time, defaulting to an empty string when absent.
            let time = block
                .select(&time_selector)
                .next()
                .map(|time| time.text().collect::<String>())
                .unwrap_or_default();

            // Combine the date and time into a single kickoff string.
            let kickoff = format!("{current_date} {time}").trim().to_string();
            // A match has been played if a score element is present; otherwise it
            // is scheduled only when a precise day and time are known.
            let status = if block.select(&score_selector).next().is_some() {
                MatchStatus::Finished
            } else if MatchDate::parse(&current_date, &time).is_some() {
                MatchStatus::Scheduled
            } else {
                MatchStatus::Tbd
            };

            // Build the event and append it to the result list.
            let id = ((day_num as usize - 1) * 7 + index) as u16;
            events.push(Event {
                id,
                team: build_team(clubs[0]),
                opponent: build_team(clubs[1]),
                kickoff,
                status,
            });
            index += 1;
        }
        Ok(events)
    }

    pub fn get_calendar_events(&self, day_num: u8) -> Result<String, FetchError> {
        self.fetch_day(&self.calendar_base_url(), day_num)
    }

    fn fetch_day(&self, base_url: &str, day_num: u8) -> Result<String, FetchError> {
        let fetcher = WebPageFetcher;
        fetcher.fetch(&format!("{base_url}{day_num}"))
    }

    fn fetch_all_events_from(
        &self,
        base_url: &str,
    ) -> Result<Vec<Event>, Box<dyn std::error::Error + Send + Sync>> {
        let mut events = Vec::new();
        for day_num in 1..=MAX_JOURNEES {
            match self.fetch_day(base_url, day_num) {
                Ok(html) => events.extend(self.parse_events(&html, day_num)?),
                Err(FetchError::NotFound) => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(events)
    }
}

fn build_team(el: ElementRef<'_>) -> Team {
    // Read the visible club name from the element text.
    let name = el.text().collect::<String>().trim().to_string();
    // Derive the club id from the last segment of its href (the slug).
    let id = el
        .value()
        .attr("href")
        .and_then(|href| href.rsplit('/').next())
        .unwrap_or("")
        .to_string();
    // Normalize the raw name to a known TeamType, falling back to Unknown.
    let team_type = TeamType::from_name(&name).unwrap_or(TeamType::Unknown);
    Team {
        id,
        name,
        team_type,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    const FIXTURE_PATH: &str = "src/parse/fixtures/lnr.html";
    const PLAYED_FIXTURE_PATH: &str = "src/parse/fixtures/lnr_played.html";

    fn load_fixture(path: &str) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|_| {
            panic!(
                "fixture missing: run `cargo test capture_lnr_fixture -- --ignored` first to fetch and save it"
            )
        })
    }

    /// Fetches real LNR HTML pages and saves them as local fixtures. Ignored by
    /// default; run explicitly with `cargo test capture_lnr_fixture -- --ignored`
    /// to (re)generate the fixtures.
    #[test]
    #[ignore]
    fn capture_lnr_fixture() {
        let parser = LnrParser::new("2026-2027");
        std::fs::create_dir_all("src/parse/fixtures").unwrap();
        let scheduled = parser.get_calendar_events(6).unwrap();
        std::fs::write(FIXTURE_PATH, scheduled).unwrap();
        let played = parser.get_calendar_events(1).unwrap();
        std::fs::write(PLAYED_FIXTURE_PATH, played).unwrap();
    }

    /// Debugging aid: loads the fixture and prints the first 5 matched rows.
    #[test]
    fn inspect_lnr_rows() {
        let raw = load_fixture(FIXTURE_PATH);
        let doc = Html::parse_document(&raw);
        let selector = Selector::parse(EVENT_SELECTOR).unwrap();
        for row in doc.select(&selector).take(5) {
            println!("{}", row.html());
        }
    }

    /// Parses the fixture and asserts that it yields at least one event.
    #[test]
    fn parse_lnr_fixture() {
        let raw = load_fixture(FIXTURE_PATH);
        let parser = LnrParser::new("2026-2027");
        let events = parser.parse(&raw).unwrap();
        println!("parsed {} events", events.len());
        for event in &events {
            println!("{event:#?}");
        }
        assert!(!events.is_empty());
    }

    /// Parses the fixture and checks exact clubs, ids, kickoff and status for the
    /// first and last events.
    #[test]
    fn parses_clubs_and_time_from_fixture() {
        let raw = load_fixture(FIXTURE_PATH);
        let parser = LnrParser::new("2026-2027");
        let events = parser.parse(&raw).unwrap();

        assert_eq!(events.len(), 7);
        let first = &events[0];
        assert_eq!(first.id, 0);
        assert_eq!(first.team.name, "Stade Français Paris");
        assert_eq!(first.team.team_type, TeamType::StadeFrancaisParis);
        assert_eq!(first.team.id, "paris");
        assert_eq!(first.opponent.name, "Montpellier Hérault Rugby");
        assert_eq!(first.opponent.team_type, TeamType::MontpellierHeraultRugby);
        assert_eq!(first.opponent.id, "montpellier");
        assert_eq!(first.kickoff, "samedi 10 octobre 14h30");
        assert_eq!(first.status, MatchStatus::Scheduled);

        let last = &events[6];
        assert_eq!(last.id, 6);
        assert_eq!(last.team.name, "Stade Toulousain");
        assert_eq!(last.team.team_type, TeamType::StadeToulousain);
        assert_eq!(last.team.id, "toulouse");
        assert_eq!(last.opponent.name, "Aviron Bayonnais");
        assert_eq!(last.opponent.team_type, TeamType::AvironBayonnais);
        assert_eq!(last.opponent.id, "bayonne");
        assert_eq!(last.kickoff, "dimanche 11 octobre 21h05");
        assert_eq!(last.status, MatchStatus::Scheduled);
    }

    /// Parses the "played" fixture and asserts every match is `Finished`.
    #[test]
    fn parses_played_matches_as_finished() {
        let raw = load_fixture(PLAYED_FIXTURE_PATH);
        let parser = LnrParser::new("2026-2027");
        let events = parser.parse(&raw).unwrap();

        assert_eq!(events.len(), 7);
        assert!(events
            .iter()
            .all(|event| event.status == MatchStatus::Finished));
    }

    fn respond(listener: &TcpListener, status_line: &str, body: &str) {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            let n = stream.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
            if request.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 {status_line}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    }

    fn serve_journees(days: usize, body: String) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}/");
        let handle = thread::spawn(move || {
            for _ in 0..days {
                respond(&listener, "200 OK", &body);
            }
            respond(&listener, "404 Not Found", "");
        });
        (base_url, handle)
    }

    /// `fetch_all_events_from` stops at the first 404; serving 2 journées of 7
    /// events yields 14 events.
    #[test]
    fn fetch_all_events_stops_on_not_found() {
        let body = load_fixture(FIXTURE_PATH);
        let (base_url, handle) = serve_journees(2, body);

        let parser = LnrParser::new("2026-2027");
        let events = parser.fetch_all_events_from(&base_url).unwrap();

        handle.join().unwrap();
        assert_eq!(events.len(), 14);
    }
}
