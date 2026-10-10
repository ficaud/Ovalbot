//! The parser extension point: any feed format (ics, html, ...) implements
//! [`EventParser`] and produces [`crate::Event`]s.

pub mod lnr;

pub use lnr::LnrParser;

use crate::Event;
use crate::ParseError;

pub trait EventParser: Send + Sync {
    fn parse(&self, raw: &str) -> Result<Vec<Event>, ParseError>;
    fn parse_all(&self) -> Result<Vec<Event>, Box<dyn std::error::Error + Send + Sync>>;
}
