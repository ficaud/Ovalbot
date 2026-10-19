//! Typed errors raised while parsing a calendar feed.

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid calendar data: {0}")]
    InvalidData(String),
    #[error("missing field: {0}")]
    MissingField(&'static str),
}

/// Fetch failures while downloading an LNR page.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("page not found")]
    NotFound,
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
}
