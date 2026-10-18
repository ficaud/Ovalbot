use crate::app::clubs::{
    larochelle::{parse_match::LarochellMatchParser, parse_seat::LarochellSeatParser},
    parsers::{ParseMatch, ParseSeat},
};
use crate::controller::html_extract::FetchHtml;
use crate::core::club::Club;
use crate::core::encounter::{Encounter, MatchNature};
use crate::core::seat::Seat;
use crate::interface::curl::proxy::ProxyMode;
/// This module is the top-level API of the parser crate: it retrieves the match list and
/// the resale seats for the supported club. It holds no persistent state and performs no
/// notification — the caller (OvalBot) owns matching, dedup and notifications.
use crate::interface::curl::web::WebClient;

/// Fetches encounters with seats for a given club and match nature: parse the match list
/// (each card carries its resale link when open), then fetch and parse the seat pages.
///
/// # Arguments
/// * `club` - The club to fetch matches for
/// * `match_type` - The nature of the matches to fetch (Rugby filters out other events)
/// # Returns
/// A list of encounters with their seats information populated
pub fn get_seats_from_matches(club: Club, match_type: MatchNature) -> Vec<Encounter> {
    let mut client = WebClient::new(ProxyMode::Rotating);

    // Parse the match list from the ticketing home page.
    let matches = get_matches_from_type_and_club(match_type, club);

    // Keep the same proxy for the seat pages to stay consistent between the
    // match list and the individual resale pages.
    client.set_proxy_mode(ProxyMode::Sticky);
    get_encounters_with_seats(matches, &client)
}

/// Internal function to fetch seats for a list of encounters, given a client to fetch HTML content.
///
/// # Arguments
/// * `matches` - The list of encounters to retrieve seats for
/// * `client` - The client to use for fetching HTML content
/// # Returns
/// A list of encounters with their seats information populated
fn get_encounters_with_seats(matches: Vec<Encounter>, client: &impl FetchHtml) -> Vec<Encounter> {
    matches
        .into_iter()
        .map(|mut encounter| {
            if let Some(link) = encounter.resale_link.clone() {
                match client.get_html(&link) {
                    Ok(html) => encounter.set_seats(get_seats(&html, encounter.clone())),
                    Err(e) => log::error!("Error fetching {}: {}", link, e),
                }
            } else {
                encounter.set_seats(Vec::new());
            }
            encounter
        })
        .collect()
}

/// Internal function to parse the match list for a given club and filter by match nature.
///
/// # Arguments
/// * `club` - The club to fetch matches for
/// * `client` - The client to use for fetching HTML content
/// * `match_type` - Filter to return only matches of a specific nature
/// # Returns
/// A list of encounters matching the specified criteria
fn get_matches(club: &Club, client: &impl FetchHtml, match_type: MatchNature) -> Vec<Encounter> {
    // SR only: the Stade Rochelais match-list parser is the only implementation.
    let parser: &dyn ParseMatch = &LarochellMatchParser;

    // Step 1: fetch the HTML of the club's ticketing home page.
    let content = client.get_html(club.get_url());

    // Step 2: parse the HTML content to find matches (encounters) information.
    let matches = parser.parse_match(&content.unwrap_or_default());

    // Step 3: filter by nature.
    matches
        .into_iter()
        .filter(|encounter| encounter.nature == match_type)
        .collect()
}

/// Internal function to fetch matches for a given club and match nature.
///
/// # Arguments
/// * `match_type` - Filter to return only matches of a specific nature
/// * `club` - The club to fetch matches for
/// # Returns
/// A list of encounters matching the specified criteria
fn get_matches_from_type_and_club(match_type: MatchNature, club: Club) -> Vec<Encounter> {
    let client = WebClient::new(ProxyMode::Rotating);
    get_matches(&club, &client, match_type)
}

/// Internal function to fetch seats for a given encounter from its resale page HTML.
///
/// # Arguments
/// * `html` - The HTML content to parse for seat information
/// * `encounter` - The encounter for which to fetch seats information
/// # Returns
/// A list of seats associated with the given encounter
fn get_seats(html: &str, encounter: Encounter) -> Vec<Seat> {
    // SR only: the Stade Rochelais seat parser is the only implementation.
    let parser: &dyn ParseSeat = &LarochellSeatParser;
    parser.parse_seat(html, encounter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::club::ClubType;
    use crate::interface::club_manager;

    const MATCH_LIST: &str = include_str!("../app/clubs/larochelle/fixtures/match_list.html");
    const SEAT_PAGE: &str = include_str!("../app/clubs/larochelle/fixtures/seat_page.html");

    struct Stub {
        html: Result<String, String>,
    }

    impl FetchHtml for Stub {
        fn get_html(&self, _url: &str) -> Result<String, String> {
            self.html.clone()
        }
    }

    fn ok(html: &'static str) -> Stub {
        Stub {
            html: Ok(html.to_string()),
        }
    }

    fn failing() -> Stub {
        Stub {
            html: Err("connection reset".to_string()),
        }
    }

    #[test]
    fn get_matches_filters_by_nature() {
        let club = club_manager::stade_rochelais();
        let rugby = get_matches(&club, &ok(MATCH_LIST), MatchNature::Rugby);
        assert_eq!(rugby.len(), 5);
        assert!(rugby.iter().all(|e| e.nature == MatchNature::Rugby));

        let basket = get_matches(&club, &ok(MATCH_LIST), MatchNature::Basketball);
        assert_eq!(basket.len(), 7);

        let other = get_matches(&club, &ok(MATCH_LIST), MatchNature::Other);
        assert_eq!(other.len(), 19);
    }

    #[test]
    fn get_matches_returns_empty_on_fetch_error() {
        let club = club_manager::stade_rochelais();
        let empty = get_matches(&club, &failing(), MatchNature::Rugby);
        assert!(empty.is_empty());
    }

    #[test]
    fn get_encounters_with_seats_populates_seats_from_resale_page() {
        let encounter = Encounter::new(
            ClubType::StadeRochelais,
            "REVENTE - ÉVREUX".to_string(),
            "vendredi 23 octobre à 20h00".to_string(),
            MatchNature::Other,
            Some("https://billetterie.staderochelais.com/fr/product/1368/x".to_string()),
        );
        let out = get_encounters_with_seats(vec![encounter], &ok(SEAT_PAGE));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].seats.as_ref().map(|s| s.len()), Some(3));
    }

    #[test]
    fn get_encounters_without_resale_link_get_empty_seats() {
        let encounter = Encounter::new(
            ClubType::StadeRochelais,
            "AVIRON BAYONNAIS".to_string(),
            "dimanche 8 novembre à 21h05".to_string(),
            MatchNature::Rugby,
            None,
        );
        let out = get_encounters_with_seats(vec![encounter], &ok(MATCH_LIST));
        assert_eq!(out[0].seats.as_ref().map(|s| s.len()), Some(0));
    }

    #[test]
    fn get_encounters_keep_seats_unset_on_fetch_error() {
        let encounter = Encounter::new(
            ClubType::StadeRochelais,
            "UNION BORDEAUX BÈGLES".to_string(),
            "dimanche 25 octobre à 21h05".to_string(),
            MatchNature::Rugby,
            Some("https://billetterie.staderochelais.com/fr/product/1357/x".to_string()),
        );
        let out = get_encounters_with_seats(vec![encounter], &failing());
        assert_eq!(out.len(), 1);
        assert!(out[0].seats.is_none());
        assert!(out[0].resale_link.is_some());
    }
}
