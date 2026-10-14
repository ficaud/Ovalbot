use std::fmt;

use serde::Serialize;

use crate::common::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Email(String);

impl Email {
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

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Email {
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
