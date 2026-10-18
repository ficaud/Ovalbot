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

#[cfg(test)]
mod tests {
    use super::*;

    // Captured live from https://billetterie.staderochelais.com/fr on 2026-10-18.
    const FIXTURE: &str = include_str!("fixtures/match_list.html");

    #[test]
    fn fixture_extracts_all_encounters() {
        let encounters = parse_match(FIXTURE);
        assert_eq!(encounters.len(), 31);

        // The dedup filter leaves no duplicate (title, date) pair.
        let mut unique = HashSet::new();
        assert!(
            encounters
                .iter()
                .all(|e| unique.insert((e.title.clone(), e.date.clone())))
        );
    }

    #[test]
    fn fixture_rugby_encounters() {
        let encounters = parse_match(FIXTURE);
        let rugby: Vec<&Encounter> = encounters
            .iter()
            .filter(|e| e.nature == MatchNature::Rugby)
            .collect();
        assert_eq!(rugby.len(), 5);
        let titles: Vec<&str> = rugby.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "UNION BORDEAUX BÈGLES",
                "AVIRON BAYONNAIS",
                "USA PERPIGNAN",
                "CASTRES OLYMPIQUE",
                "SECTION PALOISE"
            ]
        );
        assert_eq!(rugby[0].date, "dimanche 25 octobre à 21h05");
    }

    #[test]
    fn fixture_sport_nature_counts() {
        let encounters = parse_match(FIXTURE);
        let count = |nature: MatchNature| encounters.iter().filter(|e| e.nature == nature).count();
        // 7 "BASKET" badges, the rest: rugby (5), other events (19).
        assert_eq!(count(MatchNature::Basketball), 7);
        assert_eq!(count(MatchNature::Other), 19);
    }

    #[test]
    fn fixture_resale_links() {
        let encounters = parse_match(FIXTURE);
        let with_link: Vec<(&str, &str)> = encounters
            .iter()
            .filter_map(|e| {
                e.resale_link
                    .as_ref()
                    .map(|link| (e.title.as_str(), link.as_str()))
            })
            .collect();
        assert_eq!(
            with_link,
            vec![
                (
                    "ÉVREUX",
                    "https://billetterie.staderochelais.com/fr/product/1368/revente_j6_23_oct_stade_rochelais_evreux"
                ),
                (
                    "UNION BORDEAUX BÈGLES",
                    "https://billetterie.staderochelais.com/fr/product/1357/revente_j7_24_25_oct_stade_rochelais_ubb"
                ),
                (
                    "DENAIN",
                    "https://billetterie.staderochelais.com/fr/product/1371/revente_j8_06_nov_stade_rochelais_denain"
                ),
            ]
        );
    }

    #[test]
    fn badge_takes_priority_over_title_inference() {
        let html = r#"<ul>
            <li class="cards-grid__item">
                <h3 class="title">STADE ROCHELAIS - ÉVREUX</h3>
                <span class="date">samedi 6 juin 2026 à 14h00</span>
                <span class="badge-event">BASKET</span>
            </li>
            <li class="cards-grid__item">
                <h3 class="title">STADE ROCHELAIS X</h3>
                <span class="date">dimanche 7 juin</span>
            </li>
            <li class="cards-grid__item">
                <h3 class="title">REPAS D'AVANT-MATCH</h3>
                <span class="date">dimanche 7 juin</span>
                <span class="badge-event">REPAS</span>
            </li>
        </ul>"#;
        let encounters = parse_match(html);
        assert_eq!(encounters.len(), 3);
        assert_eq!(encounters[0].nature, MatchNature::Basketball);
        assert_eq!(encounters[1].nature, MatchNature::Rugby);
        assert_eq!(encounters[2].nature, MatchNature::Other);
    }

    #[test]
    fn resale_anchor_href_is_made_absolute() {
        let html = r#"<ul><li class="cards-grid__item">
            <h3 class="title">AVIRON BAYONNAIS</h3>
            <span class="date">dimanche 8 novembre à 21h05</span>
            <span class="badge-event">RUGBY</span>
            <a href="/fr/product/1357/revente_x" class="btn-resale btn">Revente</a>
        </li></ul>"#;
        let encounters = parse_match(html);
        assert_eq!(
            encounters[0].resale_link.as_deref(),
            Some("https://billetterie.staderochelais.com/fr/product/1357/revente_x")
        );
    }

    #[test]
    fn resale_button_without_href_is_ignored() {
        // Only `<a class="btn-resale">` counts; the disabled `<button>` variant
        // carries no offer.
        let html = r#"<ul><li class="cards-grid__item">
            <h3 class="title">AVIRON BAYONNAIS</h3>
            <span class="date">dimanche 8 novembre à 21h05</span>
            <button class="btn-resale btn unavailable">Revente</button>
        </li></ul>"#;
        let encounters = parse_match(html);
        assert_eq!(encounters[0].resale_link, None);
    }

    #[test]
    fn duplicates_and_titleless_cards_are_skipped() {
        let html = r#"<ul>
            <li class="cards-grid__item">
                <h3 class="title">CASTRES OLYMPIQUE</h3>
                <span class="date">week-end du 19/20 décembre</span>
                <span class="badge-event">RUGBY</span>
            </li>
            <li class="cards-grid__item">
                <h3 class="title">CASTRES OLYMPIQUE</h3>
                <span class="date">week-end du 19/20 décembre</span>
                <span class="badge-event">RUGBY</span>
            </li>
            <li class="cards-grid__item"><span class="date">samedi 6 juin</span></li>
        </ul>"#;
        let encounters = parse_match(html);
        assert_eq!(encounters.len(), 1);
        assert_eq!(encounters[0].title, "CASTRES OLYMPIQUE");
    }

    #[test]
    fn empty_document_yields_no_encounters() {
        assert!(parse_match("<html><body></body></html>").is_empty());
    }
}
