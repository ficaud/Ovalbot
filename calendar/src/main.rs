use calendar::parse::{EventParser, LnrParser};
use calendar::schedule::{for_team, upcoming};
use calendar::TeamType;

fn main() {
    let parser = LnrParser::new("2026-2027");
    let events = parser.parse_all().expect("failed to fetch all events");

    println!("fetched {} events", events.len());

    let upcoming = upcoming(&events);
    let rochelais = for_team(&upcoming, TeamType::StadeRochelais, true);
    println!("{} upcoming Stade Rochelais matches", rochelais.len());
    for event in &rochelais {
        println!("{:?}", event);
    }
}
