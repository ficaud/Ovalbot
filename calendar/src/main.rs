use calendar::parse::{EventParser, LnrParser};

fn main() {
    let parser = LnrParser::new("2026-2027");
    let events = parser.parse_all().expect("failed to fetch all events");

    println!("fetched {} events", events.len());
    for event in &events {
        println!("{event:#?}");
    }
}
