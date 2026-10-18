use crate::{
    app::clubs::parsers::ParseSeat,
    core::{
        encounter::Encounter,
        seat::{Seat, SeatComposition, SeatId, SeatInfo},
    },
};
use scraper::{Html, Selector};
/// This module contains the implementation of the `ParseSeat` trait for the Stade Rochelais website,
/// allowing the application to parse seat information from the HTML content of the Stade Rochelais ticketing pages.
use std::collections::HashMap;

/// Private module containing the JSON structures used for deserializing the seat information from the HTML content
/// of the Stade Rochelais website.
mod json {
    use serde::Deserialize;
    use std::collections::HashMap;

    #[derive(Deserialize)]
    pub struct Pack {
        pub pack_id: u64,
        pub category_id: Option<String>,
        pub tickets: HashMap<String, Ticket>,
        pub amount_by_ticket: String,
    }

    #[derive(Deserialize)]
    pub struct Ticket {
        pub seat_info: String,
    }

    #[derive(Deserialize)]
    pub struct PageSettings {
        pub resale: Option<ResaleSettings>,
    }

    #[derive(Deserialize)]
    pub struct ResaleSettings {
        pub attributes: HashMap<String, ResaleAttribute>,
    }

    #[derive(Deserialize)]
    pub struct ResaleAttribute {
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
                    preview_url: None,
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
    fn parse_seat(&self, html: &str, encounter: Encounter) -> Vec<Seat> {
        parse_seat(html, encounter)
    }
}
