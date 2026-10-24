//! Top-level API of the parser crate: the two fetches OvalBot needs from the
//! billetterie — the home page (discovery of the cards and their resale
//! links) and a single resale product page (the seats). It holds no persistent
//! state and performs no notification — the caller (OvalBot) owns matching,
//! dedup and notifications.

use crate::app::clubs::{
    larochelle::{parse_match::LarochellMatchParser, parse_seat::LarochellSeatParser},
    parsers::{ParseMatch, ParseSeat},
};
use crate::controller::html_extract::FetchHtml;
use crate::core::club::Club;
use crate::core::encounter::Encounter;
use crate::core::seat::Seat;
use crate::interface::curl::web::WebClient;

// ============================== ERRORS ==============================

/// Why a billetterie fetch could not deliver its content.
///
/// The waiting queue is distinct from a plain network failure: the shop
/// redirects there under load (an intermittent, site-side state), and the
/// caller should not treat it like a broken scan — stored links keep polling
/// and the fetch is simply retried later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanError {
    /// The HTTP fetch itself failed (timeout, connection reset, DNS…).
    Fetch(String),
    /// The billetterie served its waiting-queue page instead of the content.
    WaitingQueue,
}

/// One-line spelling of a scan error, used by the caller's logs.
impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanError::Fetch(e) => f.write_str(&format!("fetch error: {e}")),
            ScanError::WaitingQueue => f.write_str("billetterie waiting queue"),
        }
    }
}

/// Tells whether a fetched page is the billetterie waiting-queue page.
///
/// The shop redirects there under load: either the request never left the
/// queue host (`wetix-agency.fr`), or it was answered with the bare
/// "Move to waiting queue" body.
///
/// # Arguments
/// * `final_url` - The effective URL the request landed on, after redirects
/// * `body` - The fetched response body
/// # Returns
/// `true` when the page is the waiting queue, and the content must not be
/// parsed
pub fn is_waiting_queue(final_url: &str, body: &str) -> bool {
    if final_url.contains("wetix-agency.fr") {
        return true;
    }
    body.trim().eq_ignore_ascii_case("move to waiting queue")
}

// ============================== DISCOVERY ==============================

/// Fetches and parses the billetterie home page: one [`Encounter`] per card
/// (title, date, resale link when open), **without fetching any resale page**.
///
/// This is the discovery pass: it is the only place the resale links are
/// exposed, and the caller persists them so the seat pages can be polled
/// directly afterwards.
///
/// # Arguments
/// * `club` - The club whose ticketing home page to fetch
/// # Returns
/// The list of encounters found on the match list
/// # Errors
/// [`ScanError::WaitingQueue`] when the shop served its waiting-queue page,
/// [`ScanError::Fetch`] on any other fetch failure
pub fn get_home_encounters(club: Club) -> Result<Vec<Encounter>, ScanError> {
    let client = WebClient::new();
    get_matches(&club, &client)
}

/// Internal fetch of the home page through the client — testable part of
/// [`get_home_encounters`].
///
/// # Arguments
/// * `club` - The club to fetch matches for
/// * `client` - The client to use for fetching HTML content
/// # Returns
/// A list of encounters matching the specified criteria
/// # Errors
/// [`ScanError::WaitingQueue`] when the shop served its waiting-queue page,
/// [`ScanError::Fetch`] on any other fetch failure
fn get_matches(club: &Club, client: &impl FetchHtml) -> Result<Vec<Encounter>, ScanError> {
    // SR only: the Stade Rochelais match-list parser is the only implementation.
    let parser: &dyn ParseMatch = &LarochellMatchParser;

    // Step 1: fetch the HTML of the club's ticketing home page (tracked, to
    // spot the waiting-queue redirect). A failed fetch used to be silently
    // swallowed into an empty parse — it is now surfaced to the caller.
    let page = client
        .get_html_tracked(club.get_url())
        .map_err(ScanError::Fetch)?;
    if is_waiting_queue(&page.final_url, &page.body) {
        return Err(ScanError::WaitingQueue);
    }

    // Step 2: parse the HTML content to find matches (encounters) information.
    Ok(parser.parse_match(&page.body))
}

// ============================== SEAT POLLING ==============================

/// Fetches and parses the resale product page of one encounter: the direct
/// poll of a link discovered earlier (persisted by the caller in
/// `resale_links`).
///
/// # Arguments
/// * `encounter` - The encounter to poll; its `resale_link` must be set
/// # Returns
/// The seats currently on the resale page (empty when none are left)
/// # Errors
/// [`ScanError::WaitingQueue`] when the shop served its waiting-queue page,
/// [`ScanError::Fetch`] on any other fetch failure
pub fn get_seats_for_encounter(encounter: Encounter) -> Result<Vec<Seat>, ScanError> {
    if encounter.resale_link.is_none() {
        return Ok(Vec::new());
    }

    // One client serves all the links polled during the same pass (direct
    // connections — the proxy pool was removed with the batch scan).
    let client = WebClient::new();
    get_seats_with(&client, encounter)
}

/// Internal seat poll through the client — testable part of
/// [`get_seats_for_encounters`](get_seats_for_encounter).
///
/// # Errors
/// [`ScanError::WaitingQueue`] when the shop served its waiting-queue page,
/// [`ScanError::Fetch`] on any other fetch failure
fn get_seats_with(client: &impl FetchHtml, encounter: Encounter) -> Result<Vec<Seat>, ScanError> {
    let Some(link) = encounter.resale_link.clone() else {
        return Ok(Vec::new());
    };

    let page = client.get_html_tracked(&link).map_err(ScanError::Fetch)?;
    if is_waiting_queue(&page.final_url, &page.body) {
        return Err(ScanError::WaitingQueue);
    }
    Ok(get_seats(&page.body, encounter))
}

/// Internal function to parse the seat list of a resale page HTML.
///
/// # Arguments
/// * `html` - The HTML content to parse for seat information
/// * `encounter` - The encounter the resale page belongs to
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

    // Queue detection: the redirect landing on the queue host and the bare
    // queue body are both rejected; a real page and a normal URL are not.
    #[test]
    fn is_waiting_queue_detects_the_queue_page() {
        assert!(is_waiting_queue(
            "https://static-staderochelais.mutu.fa.wetix-agency.fr/?destination=x",
            "full queue page"
        ));
        assert!(is_waiting_queue(
            "https://billetterie.staderochelais.com/fr",
            "Move to waiting queue"
        ));
        assert!(!is_waiting_queue(
            "https://billetterie.staderochelais.com/fr",
            "<html>real home page</html>"
        ));
        // A shop URL merely containing the words must not trip the check.
        assert!(!is_waiting_queue(
            "https://billetterie.staderochelais.com/fr/queue-information",
            "nothing here"
        ));
    }

    // The ticketing home page returns every card, whatever its nature: the
    // list is only a catalog, the caller matches against the calendar.
    #[test]
    fn get_matches_returns_every_card() {
        let club = club_manager::stade_rochelais();
        let all = get_matches(&club, &ok(MATCH_LIST)).unwrap();
        assert_eq!(all.len(), 31);
    }

    // A fetch failure is surfaced, not silently swallowed into an empty list.
    #[test]
    fn get_matches_reports_fetch_errors() {
        let club = club_manager::stade_rochelais();
        match get_matches(&club, &failing()) {
            Err(ScanError::Fetch(e)) => assert_eq!(e, "connection reset"),
            other => panic!("expected Fetch error, got {other:?}"),
        }
    }

    // When the shop serves its waiting queue instead of the home page, the
    // caller must get the distinct error (not a silent empty list).
    #[test]
    fn get_matches_reports_the_waiting_queue() {
        let club = club_manager::stade_rochelais();
        assert!(matches!(
            get_matches(&club, &ok("Move to waiting queue")),
            Err(ScanError::WaitingQueue)
        ));
    }

    // Live 2026-10-21: the UBB card lost its RUGBY badge (replaced by its
    // resale deadline) but its resale is open — it must still be exposed with
    // its link, and its seat page must parse the live seats. The disabled
    // resale button of ÉVREUX exposes no link.
    #[test]
    fn open_resale_links_are_scanned_regardless_of_nature() {
        let club = club_manager::stade_rochelais();
        let matches = get_matches(&club, &ok(MATCH_LIST_RESALE_OPEN)).unwrap();

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
        let seats = get_seats(SEAT_PAGE_RESALE_OPEN, ubb.clone());
        let ids: BTreeSet<(u64, &str)> = seats
            .iter()
            .map(|seat| (seat.id.pack_id, seat.id.ticket_id.as_str()))
            .collect();
        assert_eq!(
            ids,
            BTreeSet::from([(33850u64, "49432"), (33853, "49435"), (33853, "49438"),])
        );
        // The single ticket of pack 33850 costs 44.00 € wherever it lands.
        assert!(
            seats
                .iter()
                .any(|seat| seat.id.pack_id == 33850 && seat.price == "44.00")
        );
    }

    // The direct poll maps a failed fetch to `ScanError::Fetch` (never a
    // silent empty list).
    #[test]
    fn get_seats_for_encounter_reports_fetch_errors() {
        let dead = Encounter::new(
            ClubType::StadeRochelais,
            "REVENTE - ÉVREUX".to_string(),
            "vendredi 23 octobre à 20h00".to_string(),
            MatchNature::Other,
            Some("https://billetterie.staderochelais.com/fr/product/9999/x".to_string()),
        );
        match get_seats_with(&failing(), dead) {
            Err(ScanError::Fetch(e)) => assert_eq!(e, "connection reset"),
            other => panic!("expected Fetch error, got {other:?}"),
        }
    }

    // The direct poll also detects the waiting queue on the product page.
    #[test]
    fn get_seats_for_encounter_reports_the_waiting_queue() {
        let queued = Encounter::new(
            ClubType::StadeRochelais,
            "REVENTE - ÉVREUX".to_string(),
            "vendredi 23 octobre à 20h00".to_string(),
            MatchNature::Other,
            Some("https://billetterie.staderochelais.com/fr/product/1368/x".to_string()),
        );
        assert!(matches!(
            get_seats_with(&ok("Move to waiting queue"), queued),
            Err(ScanError::WaitingQueue)
        ));
    }

    // A poll of an encounter without a resale link finds no seats and performs
    // no fetch.
    #[test]
    fn get_seats_for_encounter_without_link_is_empty() {
        let encounter = Encounter::new(
            ClubType::StadeRochelais,
            "AVIRON BAYONNAIS".to_string(),
            "dimanche 8 novembre à 21h05".to_string(),
            MatchNature::Rugby,
            None,
        );
        assert!(get_seats_for_encounter(encounter).unwrap().is_empty());
    }
}
