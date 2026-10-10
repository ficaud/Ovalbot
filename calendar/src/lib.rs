//! A portable, I/O-free library for sports match calendars.
//!
//! Parses calendar feeds (`.ics` or HTML) into neutral [`Event`]s and turns them into
//! domain [`Match`]es for a given [`Team`]. Provides pure helpers to classify
//! home games, filter upcoming matches and diff two schedules.

// pub mod classify;
pub mod error;
pub mod event;
pub mod fetcher;
pub mod match_date;
pub mod parse;
pub mod team;

pub use error::{FetchError, ParseError};
pub use event::Event;
pub use fetcher::WebPageFetcher;
pub use match_date::MatchDate;
pub use team::{Team, TeamType};
