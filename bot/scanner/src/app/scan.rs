use parser::core::club::Club;
use parser::core::encounter::Encounter;
/// One-shot scan of the ticketing website: fetch the match list, then the resale
/// pages of the encounters that have one, and return the encounters with their
/// seats.
///
/// Cards are not filtered by nature — the caller (OvalBot) matches them against
/// its calendar and drops the ones that are not rugby home matches.
///
/// The scan is done in a blocking task to avoid blocking the async runtime.
/// No dedup, no filter, no notification — the caller owns everything else.
///
/// # Arguments
/// * `club` - The club to scan (name + ticketing URL)
/// # Returns
/// All encounters found on the match list, with their seats when the resale is open
pub async fn run_scan(club: Club) -> Vec<Encounter> {
    tokio::task::spawn_blocking(move || {
        parser::interface::match_manager::get_seats_from_matches(club)
    })
    .await
    .unwrap_or_default()
}
