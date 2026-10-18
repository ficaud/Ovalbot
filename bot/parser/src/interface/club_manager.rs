/// This module provides the club definitions used by the scanner.
/// Currently only Stade Rochelais is supported (SR only).
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
