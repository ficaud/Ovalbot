//! The parser extension point: any feed format (ics, html, ...) implements
//! [`EventParser`] and produces [`crate::Event`]s.

pub mod lnr;

pub use lnr::LnrParser;

use crate::Event;
use crate::ParseError;

/// Parses one already-fetched page/list into events.
///
/// # Errors
/// Implementation-specific parse errors; `parse_all` additionally wraps
/// fetch errors.
pub trait EventParser: Send + Sync {
    /// Parses a single page's raw content.
    ///
    /// # Errors
    /// [`ParseError`] when the content cannot be interpreted.
    fn parse(&self, raw: &str) -> Result<Vec<Event>, ParseError>;

    /// Fetches + parses every page of the feed until exhausted (blocking).
    ///
    /// # Errors
    /// Fetch or parse failures (implementation decides what terminates the
    /// run; the LNR parser stops cleanly at the first 404).
    fn parse_all(&self) -> Result<Vec<Event>, Box<dyn std::error::Error + Send + Sync>>;
}
