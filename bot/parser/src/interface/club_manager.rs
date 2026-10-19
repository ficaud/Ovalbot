//! Club definitions used by the scanner.
//! Currently only Stade Rochelais is supported (SR only).

use crate::core::club::{Club, ClubType};

/// Returns the Stade Rochelais club entry (name + ticketing URL).
///
/// # Returns
/// A new instance of the Club struct for Stade Rochelais
pub fn stade_rochelais() -> Club {
    Club::new(
        "Stade Rochelais".to_string(),
        ClubType::StadeRochelais,
        "https://billetterie.staderochelais.com/fr".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stade_rochelais_points_to_the_ticketing_site() {
        let club = stade_rochelais();
        assert_eq!(club.name, "Stade Rochelais");
        assert_eq!(club.get_url(), "https://billetterie.staderochelais.com/fr");
    }
}
