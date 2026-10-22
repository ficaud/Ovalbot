//! Top-level API of the parser crate: retrieves the match list and the resale
//! seats for the supported club. It holds no persistent state and performs no
//! notification — the caller (OvalBot) owns matching, dedup and notifications.

use crate::app::clubs::{
    larochelle::{parse_match::LarochellMatchParser, parse_seat::LarochellSeatParser},
    parsers::{ParseMatch, ParseSeat},
};
use crate::controller::html_extract::FetchHtml;
use crate::core::club::Club;
use crate::core::encounter::Encounter;
use crate::core::seat::Seat;
use crate::interface::curl::proxy::ProxyMode;
use crate::interface::curl::web::WebClient;

/// Fetches encounters with seats for a given club: parse the match list, then
/// fetch and parse the seat pages of the encounters that have an open resale
/// link. Cards are **not** filtered by nature — classification is unreliable
/// (promotional badges overwrite the sport on nearing matches) — the caller
/// (OvalBot) matches the encounters against its calendar and drops the rest.
///
/// # Arguments
/// * `club` - The club to fetch matches for
/// # Returns
/// A list of encounters with their seats information populated when the
/// resale is open
pub fn get_seats_from_matches(club: Club) -> Vec<Encounter> {
    let mut client = WebClient::new(ProxyMode::Rotating);

    // Parse the match list from the ticketing home page.
    let matches = get_matches(&club, &client);

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

/// Internal function to parse the match list for a given club, with no nature
/// filter: every card (rugby, basketball, meals…) is returned, nature included
/// for information only.
///
/// # Arguments
/// * `club` - The club to fetch matches for
/// * `client` - The client to use for fetching HTML content
/// # Returns
/// A list of encounters matching the specified criteria
fn get_matches(club: &Club, client: &impl FetchHtml) -> Vec<Encounter> {
    // SR only: the Stade Rochelais match-list parser is the only implementation.
    let parser: &dyn ParseMatch = &LarochellMatchParser;

    // Step 1: fetch the HTML of the club's ticketing home page.
    let content = client.get_html(club.get_url());

    // Step 2: parse the HTML content to find matches (encounters) information.
    parser.parse_match(&content.unwrap_or_default())
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
    use std::collections::BTreeSet;

    use super::*;
    use crate::core::club::ClubType;
    use crate::core::encounter::MatchNature;
    use crate::interface::club_manager;

    const MATCH_LIST: &str = include_str!("../app/clubs/larochelle/fixtures/match_list.html");
    const SEAT_PAGE: &str = include_str!("../app/clubs/larochelle/fixtures/seat_page.html");
    const MATCH_LIST_RESALE_OPEN: &str =
        include_str!("../app/clubs/larochelle/fixtures/match_list_promo_badge.html");
    const SEAT_PAGE_RESALE_OPEN: &str =
        include_str!("../app/clubs/larochelle/fixtures/seat_page_resale_open.html");

    // Minimal FetchHtml double: always answers with the canned result.
    struct Stub {
        html: Result<String, String>,
    }

    impl FetchHtml for Stub {
        /// Always answers with the stub's canned result.
        fn get_html(&self, _url: &str) -> Result<String, String> {
            self.html.clone()
        }
    }

    // Setting successful: `get_html` answers the given static page.
    fn ok(html: &'static str) -> Stub {
        Stub {
            html: Ok(html.to_string()),
        }
    }

    // Failure setting: `get_html` simulates a network error.
    fn failing() -> Stub {
        Stub {
            html: Err("connection reset".to_string()),
        }
    }

    // The ticketing home page returns every card, whatever its nature: the
    // list is only a catalog, the caller matches against the calendar.
    #[test]
    fn get_matches_returns_every_card() {
        let club = club_manager::stade_rochelais();
        let all = get_matches(&club, &ok(MATCH_LIST));
        assert_eq!(all.len(), 31);
    }

    // A fetch error yields no matches at all (fail empty, not fail hard).
    #[test]
    fn get_matches_returns_empty_on_fetch_error() {
        let club = club_manager::stade_rochelais();
        let empty = get_matches(&club, &failing());
        assert!(empty.is_empty());
    }

    // Live 2026-10-21: the UBB card lost its RUGBY badge (replaced by its
    // resale deadline) but its resale is open — it must still be exposed with
    // its link, and its seat page must parse the live seats. The disabled
    // resale button of ÉVREUX exposes no link.
    #[test]
    fn open_resale_links_are_scanned_regardless_of_nature() {
        let club = club_manager::stade_rochelais();
        let matches = get_matches(&club, &ok(MATCH_LIST_RESALE_OPEN));

        let ubb = matches
            .iter()
            .find(|e| e.title == "UNION BORDEAUX BÈGLES")
            .unwrap();
        assert_eq!(
            ubb.resale_link.as_deref(),
            Some(
                "https://billetterie.staderochelais.com/fr/product/1357/revente_j7_24_25_oct_stade_rochelais_ubb"
            )
        );

        assert_eq!(
            matches
                .iter()
                .find(|e| e.title == "ÉVREUX")
                .unwrap()
                .resale_link,
            None
        );

        // The open resale page parses into 3 live seats: 1 in pack 33850 and
        // 2 in the duo pack 33853 (one seat per ticket). The seats are keyed
        // as (pack_id, ticket_id); the ticket map is a HashMap, so the duo
        // tickets are compared order-insensitively.
        let with_seats = get_encounters_with_seats(vec![ubb.clone()], &ok(SEAT_PAGE_RESALE_OPEN));
        let ids: BTreeSet<(u64, &str)> = with_seats[0]
            .seats
            .as_ref()
            .unwrap()
            .iter()
            .map(|seat| (seat.id.pack_id, seat.id.ticket_id.as_str()))
            .collect();
        assert_eq!(
            ids,
            BTreeSet::from([(33850u64, "49432"), (33853, "49435"), (33853, "49438"),])
        );
        // The single ticket of pack 33850 costs 44.00 € wherever it lands.
        assert!(
            with_seats[0]
                .seats
                .as_ref()
                .unwrap()
                .iter()
                .any(|seat| seat.id.pack_id == 33850 && seat.price == "44.00")
        );
    }

    // Encounters with an open resale link get their seats parsed from that page.
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

    // No resale link → empty (not `None`) seat list.
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

    // A fetch error leaves `seats` untouched (`None`), not emptied.
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
