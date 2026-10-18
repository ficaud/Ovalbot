use parser::core::club::Club;
use parser::core::encounter::{Encounter, MatchNature};
/// One-shot scan of the ticketing website: fetch the match list, then the resale pages
/// of the encounters that have one, and return the encounters with their seats.
///
/// The scan is done in a blocking task to avoid blocking the async runtime.
/// No dedup, no filter, no notification — the caller owns everything else.
///
/// # Arguments
/// * `club` - The club to scan (name + ticketing URL)
/// * `match_type` - The nature of matches to keep (Rugby filters out other events)
/// # Returns
/// All encounters found on the match list, with their seats when the resale is open
pub async fn run_scan(club: Club, match_type: MatchNature) -> Vec<Encounter> {
    tokio::task::spawn_blocking(move || {
        parser::interface::match_manager::get_seats_from_matches(club, match_type)
    })
    .await
    .unwrap_or_default()
}
