/// This module defines the Club struct and the ClubType enum, which represent rugby clubs and their types.
///
/// ClubType is the type of club parsed by the application.
/// Currently only Stade Rochelais is implemented; new clubs would be added here
/// together with the parser modules in `app/clubs/`.
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
