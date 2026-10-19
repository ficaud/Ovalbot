//! Seat-page parser for Stade Rochelais: reads the resale seats out of a
//! billetterie product page (pack JSON blobs + Drupal settings for categories).

use crate::{
    app::clubs::parsers::ParseSeat,
    core::{
        encounter::Encounter,
        seat::{Seat, SeatComposition, SeatId, SeatInfo},
    },
};
use scraper::{Html, Selector};
use std::collections::HashMap;

/// Private module containing the JSON structures used for deserializing the seat information from the HTML content
/// of the Stade Rochelais website.
mod json {
    use serde::Deserialize;
    use std::collections::HashMap;

    /// One resale pack: its id, category, tickets and unit price.
    #[derive(Deserialize)]
    pub struct Pack {
        /// Pack id from the billetterie (stable seat-group identifier).
        pub pack_id: u64,
        /// Category id into the Drupal settings map, if any.
        pub category_id: Option<String>,
        /// Ticket id -> seat description (HTML fragment).
        pub tickets: HashMap<String, Ticket>,
        /// Unit price as displayed, e.g. `"32.00"`.
        pub amount_by_ticket: String,
    }

    /// One ticket inside a pack.
    #[derive(Deserialize)]
    pub struct Ticket {
        /// HTML fragment describing the seat ("Rang H • Siège 57").
        pub seat_info: String,
    }

    /// Root of the `drupal-settings-json` script.
    #[derive(Deserialize)]
    pub struct PageSettings {
        /// Resale settings, absent on non-resale pages.
        pub resale: Option<ResaleSettings>,
    }

    /// Category id -> attribute entries of the resale settings.
    #[derive(Deserialize)]
    pub struct ResaleSettings {
        /// Category entries keyed by id.
        pub attributes: HashMap<String, ResaleAttribute>,
    }

    /// One resale category (id -> human name, e.g. "Pelouse").
    #[derive(Deserialize)]
    pub struct ResaleAttribute {
        /// Display name of the category.
        pub name: String,
    }
}

/// The `LarochellSeatParser` struct implements the `ParseSeat` trait, providing functionality to parse seat information from HTML content specific to Stade Rochelais.
pub struct LarochellSeatParser;

/// Extracts the resale category names from the page's Drupal settings JSON.
///
/// # Arguments
/// * `document` - The parsed HTML document to extract the category names from
/// # Returns
/// A map of category id -> category name (e.g. "Pelouse", "Tribune Or"); empty when absent
fn extract_category_names(document: &Html) -> HashMap<String, String> {
    let script_selector =
        Selector::parse("script[data-drupal-selector='drupal-settings-json']").unwrap();
    let settings_json = match document.select(&script_selector).next() {
        Some(script) => script.text().collect::<String>(),
        None => return HashMap::new(),
    };
    let settings: json::PageSettings = match serde_json::from_str(&settings_json) {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };
    settings
        .resale
        .map(|r| r.attributes.into_iter().map(|(k, v)| (k, v.name)).collect())
        .unwrap_or_default()
}

/// Parses the provided HTML content to extract seat information for a given encounter related to Stade Rochelais.
/// It looks for specific HTML elements that contain the seat information and constructs a list of `Seat`
/// instances based on the extracted data, including the seat composition and actions that can be performed on the seat (like adding to cart).
///
/// The function also extracts necessary context information from the page, such as AJAX URLs and form tokens, which are required for performing actions on the seats.
///
/// # Arguments
/// * `html` - A string slice containing the HTML content to be parsed
/// * `encounter` - The encounter for which the seat information is being parsed (used to determine the correct parser based on club type)
/// # Returns
/// A vector of `Seat` instances representing the seats extracted from the HTML content, including their composition and actions that can be performed on them (like adding to cart)
pub fn parse_seat(html: &str, _encounter: Encounter) -> Vec<Seat> {
    // Parse the HTML content using the scraper crate to create a document object
    let document = Html::parse_document(html);

    // Define the CSS selector for the elements containing seat information
    let pack_selector = Selector::parse("div.js-listing-plain[data-json]").unwrap();
    let span_selector = Selector::parse("span").unwrap();

    // Extract the category names from the page context
    let category_names = extract_category_names(&document);

    // Use a vector to store the extracted seats information
    let mut seats = Vec::new();

    // The seat information is contained in elements with the class `js-listing-plain` and a `data-json` attribute, which contains a JSON string with the seat details.
    // The function iterates over each of these elements, parses the JSON data to extract the seat information, and constructs `Seat` instances based on the extracted data,
    // including the seat composition and actions that can be performed on the seat (like adding to cart).
    for pack_el in document.select(&pack_selector) {
        let data_json = match pack_el.value().attr("data-json") {
            Some(j) => j,
            None => continue,
        };
        let pack: json::Pack = match serde_json::from_str(data_json) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("Failed to parse pack JSON: {}", e);
                continue;
            }
        };

        for (ticket_id, ticket) in &pack.tickets {
            let seat_doc = Html::parse_fragment(&ticket.seat_info);
            let seat_text = seat_doc
                .select(&span_selector)
                .next()
                .map(|el| el.text().collect::<String>())
                .unwrap_or_default();

            // Determine the category of the seat based on the category ID from the pack and the category names extracted from the page context
            let category = pack
                .category_id
                .as_deref()
                .and_then(|id| category_names.get(id))
                .cloned()
                .unwrap_or_default();

            // Construct a `Seat` instance based on the extracted seat information
            seats.push(Seat {
                id: SeatId {
                    pack_id: pack.pack_id,
                    ticket_id: ticket_id.clone(),
                },
                seat_info: SeatInfo {
                    full_name: seat_text.clone(),
                    composition: get_seat_composition(&seat_text, &category),
                },
                price: pack.amount_by_ticket.clone(),
            });
        }
    }

    seats
}

/// Internal function to parse seat composition from seat information and category.
///
/// The seat composition is the detailed breakdown of a seat's location and category
/// extracted from the seat information string and the category name.
///
/// The function splits the seat information into parts and identifies the
/// access type, row, and seat number based on specific prefixes in the text.
///
/// example : "Accès Tribune Or • Rang 5 • Siège 12" will be parsed into access: "Tribune Or", row: "5", seat_number: 12
///
/// # Arguments
/// * `seat_info` - The seat information string to parse
/// * `category` - The category of the seat
/// # Returns
/// A `SeatComposition` struct containing the parsed seat composition information
fn get_seat_composition(seat_info: &str, category: &str) -> SeatComposition {
    let parts: Vec<&str> = seat_info.split('•').map(|s| s.trim()).collect();
    let mut composition = SeatComposition {
        category: category.to_string(),
        bloc: String::new(),
        row: String::new(),
        seat_number: 0,
    };

    for part in parts {
        if part.starts_with("Bloc") {
            composition.bloc = part.replacen("Bloc", "", 1).trim().to_string();
        } else if part.starts_with("Rang") {
            composition.row = part.replacen("Rang", "", 1).trim().to_string();
        } else if part.starts_with("Siège")
            && let Ok(num) = part.replacen("Siège", "", 1).trim().parse::<u64>()
        {
            composition.seat_number = num;
        }
    }

    composition
}

/// Implementation of the `ParseSeat` trait for the `LarochellSeatParser`,
/// allowing it to parse seat information from HTML content specific to Stade Rochelais.
impl ParseSeat for LarochellSeatParser {
    /// Delegates to the free function (see its doc for the parsing rules).
    fn parse_seat(&self, html: &str, encounter: Encounter) -> Vec<Seat> {
        parse_seat(html, encounter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::club::ClubType;

    // Captured live from a resale page (open basketball resale) on 2026-10-18:
    // https://billetterie.staderochelais.com/fr/product/1368/revente_j6_23_oct_stade_rochelais_evreux
    const FIXTURE: &str = include_str!("fixtures/seat_page.html");

    // Minimal encounter fixture passed through to `parse_seat` (unused).
    fn sample_encounter() -> Encounter {
        Encounter::new(
            ClubType::StadeRochelais,
            "REVENTE - ÉVREUX".to_string(),
            "vendredi 23 octobre à 20h00".to_string(),
            crate::core::encounter::MatchNature::Other,
            None,
        )
    }

    // The committed fixture page yields the 3 known seats with their prices/categories.
    #[test]
    fn fixture_seats_parsed() {
        let seats = parse_seat(FIXTURE, sample_encounter());
        assert_eq!(seats.len(), 3);

        // Look seats up by ticket id: the tickets map is a HashMap, so the
        // iteration order is not stable.
        let by_ticket_id: HashMap<&str, &Seat> =
            seats.iter().map(|s| (s.id.ticket_id.as_str(), s)).collect();

        let s1 = by_ticket_id["49206"];
        assert_eq!(s1.id.pack_id, 33696);
        assert_eq!(s1.price, "32.00");
        assert_eq!(s1.seat_info.full_name, "Rang H • Siège 57");
        assert_eq!(s1.seat_info.composition.category, "CATÉGORIE 1");
        assert_eq!(s1.seat_info.composition.row, "H");
        assert_eq!(s1.seat_info.composition.seat_number, 57);

        let s2 = by_ticket_id["49214"];
        assert_eq!(s2.id.pack_id, 33701);
        assert_eq!(s2.price, "24.00");
        assert_eq!(s2.seat_info.composition.category, "CATÉGORIE 2");
        assert_eq!(s2.seat_info.composition.row, "C");
        assert_eq!(s2.seat_info.composition.seat_number, 8);

        let s3 = by_ticket_id["49216"];
        assert_eq!(s3.seat_info.composition.category, "CATÉGORIE 3");
        assert_eq!(s3.seat_info.composition.seat_number, 6);
    }

    // A pack whose `data-json` is garbage is dropped, valid packs still parse.
    #[test]
    fn malformed_pack_json_is_skipped() {
        let html = r#"
            <div class="js-listing-plain" data-json="not-json">invalid</div>
            <div class="js-listing-plain" data-json="{&quot;pack_id&quot;:5,&quot;category_id&quot;:&quot;9&quot;,&quot;tickets&quot;:{&quot;t1&quot;:{&quot;seat_info&quot;:&quot;<span>Pelouse &bull; Rang 5 &bull; Si&amp;#232;ge 42</span>&quot;}},&quot;amount_by_ticket&quot;:&quot;15.00&quot;}"></div>
            <script data-drupal-selector="drupal-settings-json">{"resale":{"attributes":{"9":{"name":"Pelouse"}}}}</script>
        "#;
        let seats = parse_seat(html, sample_encounter());
        assert_eq!(seats.len(), 1);
        assert_eq!(seats[0].id.pack_id, 5);
        assert_eq!(seats[0].id.ticket_id, "t1");
        assert_eq!(seats[0].price, "15.00");
        assert_eq!(seats[0].seat_info.composition.category, "Pelouse");
        assert_eq!(seats[0].seat_info.composition.row, "5");
        assert_eq!(seats[0].seat_info.composition.seat_number, 42);
    }

    // An empty page parses to zero seats (no panic on missing markup).
    #[test]
    fn page_without_settings_or_packs_yields_no_seats() {
        assert!(parse_seat("<html><body></body></html>", sample_encounter()).is_empty());
    }

    // A category id absent from the Drupal settings degrades to an empty name.
    #[test]
    fn unknown_category_id_falls_back_to_empty_name() {
        let html = r#"
            <div class="js-listing-plain" data-json="{&quot;pack_id&quot;:7,&quot;category_id&quot;:&quot;999&quot;,&quot;tickets&quot;:{&quot;t9&quot;:{&quot;seat_info&quot;:&quot;<span>Pelouse &bull; Rang 1 &bull; Si&amp;#232;ge 2</span>&quot;}},&quot;amount_by_ticket&quot;:&quot;9.00&quot;}"></div>
        "#;
        let seats = parse_seat(html, sample_encounter());
        assert_eq!(seats.len(), 1);
        assert_eq!(seats[0].seat_info.composition.category, "");
    }

    // Composition parsing: prefixes Bloc/Rang/Siège are stripped one by one.
    #[test]
    fn get_seat_composition_parses_known_parts() {
        let composition =
            get_seat_composition("Accès Tribune Or • Rang 5 • Siège 12", "Tribune Or");
        assert_eq!(composition.category, "Tribune Or");
        assert_eq!(composition.bloc, "");
        assert_eq!(composition.row, "5");
        assert_eq!(composition.seat_number, 12);

        let composition = get_seat_composition("Bloc Nord • Rang 2 • Siège 8", "CATÉGORIE 2");
        assert_eq!(composition.bloc, "Nord");
        assert_eq!(composition.row, "2");
        assert_eq!(composition.seat_number, 8);
    }

    // Missing/absent details leave zero values instead of failing.
    #[test]
    fn get_seat_composition_defaults_to_zeroes() {
        let composition = get_seat_composition("", "");
        assert_eq!(composition.row, "");
        assert_eq!(composition.seat_number, 0);

        let composition = get_seat_composition("Aucun détail", "Pelouse");
        assert_eq!(composition.row, "");
        assert_eq!(composition.seat_number, 0);
    }
}
