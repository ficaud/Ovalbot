//! A club identity, used to classify home vs away matches.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TeamType {
    StadeRochelais,
    StadeToulousain,
    RcToulon,
    RcVannes,
    AvironBayonnais,
    Racing92,
    Usap,
    AsmClermont,
    LouRugby,
    SectionPalois,
    CastresOlympique,
    MontpellierHeraultRugby,
    StadeFrancaisParis,
    UnionBordeauxBegles,
    Unknown,
}

impl TeamType {
    pub fn from_name(raw: &str) -> Option<TeamType> {
        let s = raw.trim().to_lowercase();
        match s {
            _ if s.contains("toulon") => Some(TeamType::RcToulon),
            _ if s.contains("toulouse") || s.contains("toulousain") => {
                Some(TeamType::StadeToulousain)
            }
            _ if s.contains("rochelais") || s.contains("la rochelle") => {
                Some(TeamType::StadeRochelais)
            }
            _ if s.contains("vannes") => Some(TeamType::RcVannes),
            _ if s.contains("bayonn") => Some(TeamType::AvironBayonnais),
            _ if s.contains("racing") => Some(TeamType::Racing92),
            _ if s.contains("perpignan") || s.contains("usap") => Some(TeamType::Usap),
            _ if s.contains("clermont") || s.contains("montferrand") || s.contains("asm") => {
                Some(TeamType::AsmClermont)
            }
            _ if s.contains("lyon") || s.contains("lou") => Some(TeamType::LouRugby),
            _ if s.contains("paloise") || s.contains("palois") || s.contains("pau") => {
                Some(TeamType::SectionPalois)
            }
            _ if s.contains("castres") => Some(TeamType::CastresOlympique),
            _ if s.contains("montpellier") => Some(TeamType::MontpellierHeraultRugby),
            _ if s.contains("français") || s.contains("francais") || s.contains("paris") => {
                Some(TeamType::StadeFrancaisParis)
            }
            _ if s.contains("bordeaux") || s.contains("bègles") || s.contains("begles") => {
                Some(TeamType::UnionBordeauxBegles)
            }
            _ => None,
        }
    }

    pub fn all() -> [TeamType; 14] {
        [
            TeamType::StadeRochelais,
            TeamType::StadeToulousain,
            TeamType::RcToulon,
            TeamType::RcVannes,
            TeamType::AvironBayonnais,
            TeamType::Racing92,
            TeamType::Usap,
            TeamType::AsmClermont,
            TeamType::LouRugby,
            TeamType::SectionPalois,
            TeamType::CastresOlympique,
            TeamType::MontpellierHeraultRugby,
            TeamType::StadeFrancaisParis,
            TeamType::UnionBordeauxBegles,
        ]
    }

    pub fn id(&self) -> &'static str {
        match self {
            TeamType::StadeRochelais => "stade-rochelais",
            TeamType::StadeToulousain => "stade-toulousain",
            TeamType::RcToulon => "rc-toulon",
            TeamType::RcVannes => "rc-vannes",
            TeamType::AvironBayonnais => "aviron-bayonnais",
            TeamType::Racing92 => "racing-92",
            TeamType::Usap => "usa-perpignan",
            TeamType::AsmClermont => "asm-clermont",
            TeamType::LouRugby => "lou-rugby",
            TeamType::SectionPalois => "section-paloise",
            TeamType::CastresOlympique => "castres-olympique",
            TeamType::MontpellierHeraultRugby => "montpellier-herault-rugby",
            TeamType::StadeFrancaisParis => "stade-francais-paris",
            TeamType::UnionBordeauxBegles => "union-bordeaux-begles",
            TeamType::Unknown => "unknown",
        }
    }

    pub fn from_id(id: &str) -> Option<TeamType> {
        TeamType::all().into_iter().find(|team| team.id() == id)
    }
}

impl fmt::Display for TeamType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            TeamType::StadeRochelais => "Stade Rochelais",
            TeamType::StadeToulousain => "Stade Toulousain",
            TeamType::RcToulon => "RC Toulon",
            TeamType::RcVannes => "RC Vannes",
            TeamType::AvironBayonnais => "Aviron Bayonnais",
            TeamType::Racing92 => "Racing 92",
            TeamType::Usap => "USA Perpignan",
            TeamType::AsmClermont => "ASM Clermont Auvergne",
            TeamType::LouRugby => "LOU Rugby",
            TeamType::SectionPalois => "Section Paloise",
            TeamType::CastresOlympique => "Castres Olympique",
            TeamType::MontpellierHeraultRugby => "Montpellier Hérault Rugby",
            TeamType::StadeFrancaisParis => "Stade Français Paris",
            TeamType::UnionBordeauxBegles => "Union Bordeaux Bègles",
            TeamType::Unknown => "Unknown",
        };
        write!(f, "{name}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub team_type: TeamType,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `from_name` maps distinctive club names to their `TeamType`, returning
    /// `None` for unrecognized input.
    #[test]
    fn from_name_matches_distinctive_names() {
        assert_eq!(
            TeamType::from_name("Stade Rochelais"),
            Some(TeamType::StadeRochelais)
        );
        assert_eq!(TeamType::from_name("Toulon"), Some(TeamType::RcToulon));
        assert_eq!(
            TeamType::from_name("Toulouse"),
            Some(TeamType::StadeToulousain)
        );
        assert_eq!(TeamType::from_name("LOU Rugby"), Some(TeamType::LouRugby));
        assert_eq!(TeamType::from_name("Unknown"), None);
    }

    /// `from_name` checks "toulon" before "toulouse"/"toulousain" so that both
    /// clubs resolve to the right `TeamType`.
    #[test]
    fn from_name_disambiguates_toulon_before_toulouse() {
        assert_eq!(TeamType::from_name("RC Toulon"), Some(TeamType::RcToulon));
        assert_eq!(
            TeamType::from_name("Stade Toulousain"),
            Some(TeamType::StadeToulousain)
        );
    }

    /// `from_name` is case- and accent-insensitive.
    #[test]
    fn from_name_is_case_and_accent_insensitive() {
        assert_eq!(
            TeamType::from_name("stade francais"),
            Some(TeamType::StadeFrancaisParis)
        );
        assert_eq!(
            TeamType::from_name("BORDEAUX BEGLES"),
            Some(TeamType::UnionBordeauxBegles)
        );
    }

    /// `Display` returns the canonical French club name.
    #[test]
    fn display_returns_canonical_name() {
        assert_eq!(TeamType::StadeRochelais.to_string(), "Stade Rochelais");
        assert_eq!(TeamType::AsmClermont.to_string(), "ASM Clermont Auvergne");
    }

    /// `all` returns exactly the 14 known teams and excludes `Unknown`.
    #[test]
    fn all_lists_the_fourteen_teams() {
        let teams = TeamType::all();
        assert_eq!(teams.len(), 14);
        assert!(teams.contains(&TeamType::StadeRochelais));
        assert!(!teams.contains(&TeamType::Unknown));
    }

    /// `id` and `from_id` round-trip for every known team.
    #[test]
    fn id_and_from_id_roundtrip() {
        for team in TeamType::all() {
            assert_eq!(TeamType::from_id(team.id()), Some(team));
        }
    }

    /// `from_id` returns `None` for an unknown slug.
    #[test]
    fn from_id_returns_none_for_unknown() {
        assert_eq!(TeamType::from_id("not-a-team"), None);
    }
}
