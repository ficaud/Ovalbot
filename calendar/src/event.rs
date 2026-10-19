//! Events composition for a single match.

use crate::team::Team;

/// Scheduling state of a match, as read from the LNR page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStatus {
    /// No kickoff yet (the page shows a date range or nothing).
    Tbd,
    /// A precise date+time is set ([`MatchDate`]-parsable).
    Scheduled,
    /// The match was played; it no longer appears as upcoming.
    Finished,
}

/// One calendar match: home team, away team, kickoff text and status.
///
/// `id` is the **LNR match id** (from the `feuille-de-match` link) — it's the
/// stable key shared with ticket requests and notification subscriptions. The
/// positional page index is only a fallback when the link is missing.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// LNR match id (stable across calendar re-syncs).
    pub id: u32,
    /// Home club.
    pub team: Team,
    /// Away club.
    pub opponent: Team,
    /// Kickoff line copied verbatim from the page (French, may be a range).
    pub kickoff: String,
    /// Scheduling state derived from the page markers.
    pub status: MatchStatus,
}
