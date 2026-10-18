use crate::{
    app::clubs::parsers::ParseMatch,
    core::{
        club::ClubType,
        encounter::{Encounter, MatchNature},
    },
};
use scraper::{Html, Selector};
/// This module contains the implementation of the match parser for Stade Rochelais,
/// which extracts match information from the club's website.
use std::collections::HashSet;

/// The `LarochellMatchParser` struct implements the `ParseMatch` trait, providing functionality to parse match information from HTML content specific to Stade Rochelais.
pub struct LarochellMatchParser;

/// Parses the provided HTML content to extract match information for Stade Rochelais.
/// It looks for specific HTML elements that contain the match title, date, and resale link, and constructs a list of `Encounter` instances based on the extracted data.
/// The function also ensures that duplicate matches (with the same title and date) are filtered out from the final list of encounters.
///
/// # Arguments
/// * `html` - A string slice containing the HTML content to be parsed
/// # Returns
/// A vector of `Encounter` instances representing the matches extracted from the HTML content
pub fn parse_match(html: &str) -> Vec<Encounter> {
    // Parse the HTML content using the scraper crate to create a document object
    let document = Html::parse_document(html);

    // Define CSS selectors for the relevant HTML elements containing match information
    let meeting_selector = Selector::parse("li.cards-grid__item").unwrap();
    let title_selector = Selector::parse("h3.title").unwrap();
    let date_selector = Selector::parse("span.date").unwrap();
    let link_selector = Selector::parse("a.btn-resale").unwrap();
    let badge_selector = Selector::parse("span.badge-event").unwrap();

    // Use a HashSet to track seen matches and filter out duplicates based on title and date
    let mut encounters = Vec::new();

    // Iterate over each meeting element found in the document and extract the title, date, and resale link for each match
    for meeting in document.select(&meeting_selector) {
        let title = meeting
            .select(&title_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let date = meeting
            .select(&date_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let badge = meeting
            .select(&badge_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        if !title.is_empty() {
            let nature = if badge.is_empty() {
                MatchNature::from_title(&title)
            } else {
                MatchNature::from_badge(&badge)
            };
            let resale_link = meeting
                .select(&link_selector)
                .next()
                .and_then(|el| el.value().attr("href"))
                .map(|href| {
                    format!(
                        "https://billetterie.staderochelais.com/{}",
                        href.trim_start_matches('/')
                    )
                });
            encounters.push(Encounter::new(
                ClubType::StadeRochelais,
                title,
                date,
                nature,
                resale_link,
            ));
        }
    }

    // Filter out duplicate matches based on title and date
    let mut seen = HashSet::new();
    encounters.retain(|e| seen.insert((e.title.clone(), e.date.clone())));

    encounters
}

/// Implements the `ParseMatch` trait for the `LarochellMatchParser` struct, allowing it to be used as a match parser for Stade Rochelais.
impl ParseMatch for LarochellMatchParser {
    fn parse_match(&self, html: &str) -> Vec<Encounter> {
        parse_match(html)
    }
}
