//! Rugby club identities known to the parser.
//!
//! [`ClubType`] is the club a scan targets. Only Stade Rochelais is
//! implemented today; new clubs would be added here together with their
//! parser modules under `app/clubs/`.

/// The club a scan is run for (currently a single-variant enum).
#[derive(Debug, Clone)]
pub enum ClubType {
    StadeRochelais,
}

/// Club structure that represents a rugby club, including its name, type, and the URL of its ticketing website.
#[derive(Debug, Clone)]
pub struct Club {
    pub name: String,
    pub club_type: ClubType,
    pub url: String,
}

/// Implementation of the Club struct, including a constructor and a method to get the URL of the club's ticketing website.
impl Club {
    /// Creates a new Club instance with the given name, type, and URL.
    ///
    /// # Arguments
    /// * `name` - The name of the club
    /// * `club_type` - The type of the club
    /// * `url` - The URL of the club's ticketing website
    /// # Returns
    /// A new instance of the Club struct with the provided information
    pub fn new(name: String, club_type: ClubType, url: String) -> Self {
        Self {
            name,
            club_type,
            url,
        }
    }

    /// Returns the URL of the club's ticketing website.
    ///
    /// # Returns
    /// A string slice containing the URL of the club's ticketing website
    pub fn get_url(&self) -> &str {
        &self.url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn club_new_stores_fields() {
        let club = Club::new(
            "Stade Rochelais".to_string(),
            ClubType::StadeRochelais,
            "https://example.com".to_string(),
        );
        assert_eq!(club.name, "Stade Rochelais");
        assert!(matches!(club.club_type, ClubType::StadeRochelais));
        assert_eq!(club.get_url(), "https://example.com");
    }
}
