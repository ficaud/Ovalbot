//! Events composition for a single match.

use crate::team::Team;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStatus {
    Tbd,
    Scheduled,
    Finished,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub id: u32,
    pub team: Team,
    pub opponent: Team,
    pub kickoff: String,
    pub status: MatchStatus,
}
