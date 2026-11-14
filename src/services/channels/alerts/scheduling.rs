//! Payload of the match-scheduling alert (a `Tbd → Scheduled` calendar flip),
//! shared by every channel.

/// Content of one match-scheduling alert, shared by every channel.
#[derive(Debug, Clone)]
pub struct MatchAlert {
    pub team: String,
    pub opponent: String,
    pub kickoff: String,
}
