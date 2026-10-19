//! Per-club parser contracts: one trait for the match-list page, one for the
//! seat page, so the scanner stays club-agnostic.

use crate::core::encounter::Encounter;
use crate::core::seat::Seat;

/// Parses a club's ticketing home page into encounters (one per match card).
pub trait ParseMatch {
    /// Extracts the encounters, duplicates (same title+date) removed.
    fn parse_match(&self, html: &str) -> Vec<Encounter>;
}

/// Parses one club's resale page into the seats currently offered.
pub trait ParseSeat {
    /// Extracts the seats; `encounter` identifies the page being parsed.
    fn parse_seat(&self, html: &str, encounter: Encounter) -> Vec<Seat>;
}
