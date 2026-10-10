//! Pure business logic: classify events into matches, keep upcoming games,
//! and diff two schedules to detect changes.

// use crate::event::Event;
use crate::fixture::Match;
// use crate::team::Team;

pub enum Change {
    Scheduled {
        uid: String,
        opponent: String,
        kickoff: String,
    },
    Added {
        uid: String,
        opponent: String,
    },
    Removed {
        uid: String,
    },
}

// pub fn classify(_events: &[Event], _team: &Team) -> Vec<Match> {
//     todo!("classify events into home matches for a team")
// }
//
pub fn upcoming(_matches: &[Match], _now: &str) -> Vec<Match> {
    todo!("filter and sort upcoming matches")
}

pub fn diff(_old: &[Match], _new: &[Match]) -> Vec<Change> {
    todo!("compute changes between two schedules")
}

#[cfg(test)]
mod tests {}
