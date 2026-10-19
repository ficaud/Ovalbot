//! Lightweight domain type for email addresses: parsing + normalization.

use std::fmt;

use serde::Serialize;

use crate::common::error::AppError;

/// A validated, normalized email address (trimmed, lower-cased) held inline so
/// unvalidated strings never reach the mailer or the DB by accident.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Email(String);

impl Email {
    /// Validates and normalizes a free-form input.
    ///
    /// The checks are deliberately pragmatic (no RFC 5322 grammar): non-empty,
    /// no whitespace, exactly one `@`, and a domain containing at least one
    /// dot — enough to reject `user@localhost`-style junk.
    ///
    /// # Returns
    /// The trimmed, lower-cased address, or `AppError::InvalidEmail`.
    ///
    /// # Errors
    /// `AppError::InvalidEmail` when any check fails (mapped to 400).
    pub fn parse(input: &str) -> Result<Self, AppError> {
        let trimmed = input.trim().to_lowercase();
        let valid = !trimmed.is_empty()
            && trimmed.contains('@')
            && !trimmed.contains(char::is_whitespace)
            && trimmed.split('@').count() == 2
            && trimmed.split('@').nth(1).unwrap().contains('.');
        if valid {
            Ok(Self(trimmed))
        } else {
            Err(AppError::InvalidEmail)
        }
    }

    /// The normalized address (`Parse` returns the struct itself).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Email {
    /// Displays the plain address; same as [`Email::as_str`], as a `String`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `parse` accepts a valid email and normalizes it to lowercase.
    #[test]
    fn parses_valid_email() {
        assert_eq!(
            Email::parse("User@Example.COM").unwrap().as_str(),
            "user@example.com"
        );
    }

    /// `parse` rejects an address without an `@`.
    #[test]
    fn rejects_email_without_at() {
        assert!(Email::parse("not-an-email").is_err());
    }

    /// `parse` rejects an address whose domain has no dot.
    #[test]
    fn rejects_email_without_domain_dot() {
        assert!(Email::parse("user@localhost").is_err());
    }
}
