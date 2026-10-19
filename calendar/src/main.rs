use calendar::parse::{EventParser, LnrParser};
use calendar::schedule::{for_team, upcoming};
use calendar::TeamType;

// Tiny demo binary for the calendar crate: syncs the season once and prints
// it (no server, no DB — `cargo run -p calendar`).
fn main() {
    // Create parser for the current season.
    let parser = LnrParser::new("2026-2027");
    // Fetch all events from the lnr calendar.
    let events = parser.parse_all().expect("failed to fetch all events");
    // Only retrieve the upcoming matches for the Rochelais team.
    let upcoming = upcoming(&events);
    // Filter by the Rochelais team and for the ones at home.
    let rochelais = for_team(&upcoming, TeamType::StadeRochelais, true);
    println!("{} upcoming Stade Rochelais matches", rochelais.len());
    for event in &rochelais {
        println!("{:?}", event);
    }
}
