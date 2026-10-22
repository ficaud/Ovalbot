use parser::core::club::Club;
use parser::core::encounter::Encounter;
use parser::core::seat::Seat;
use parser::interface::ScanError;

/// Discovery pass of the ticketing website: fetch and parse the match list
/// (one encounter per card, resale link attached when open) — **no resale
/// page is fetched here**. The caller persists the discovered links so the
/// seat pages can be polled directly.
///
/// Work is done in a blocking task to avoid blocking the async runtime.
/// No dedup, no filter, no notification — the caller owns everything else.
///
/// # Arguments
/// * `club` - The club to scan (name + ticketing URL)
/// # Returns
/// All encounters found on the match list
/// # Errors
/// [`ScanError::WaitingQueue`] when the shop served its waiting-queue page,
/// [`ScanError::Fetch`] on any other fetch failure
pub async fn discover(club: Club) -> Result<Vec<Encounter>, ScanError> {
    tokio::task::spawn_blocking(move || parser::interface::match_manager::get_home_encounters(club))
        .await
        .map_err(|_| ScanError::Fetch("discovery task panicked or was cancelled".to_string()))?
}

/// Direct poll of one encounter's resale product page (a link discovered
/// earlier). Returns the seats currently on resale.
///
/// Work is done in a blocking task to avoid blocking the async runtime.
///
/// # Arguments
/// * `encounter` - The encounter to poll; its `resale_link` must be set
/// # Returns
/// The seats currently on the resale page (empty when none are left)
/// # Errors
/// [`ScanError::WaitingQueue`] when the shop served its waiting-queue page,
/// [`ScanError::Fetch`] on any other fetch failure
pub async fn fetch_seats(encounter: Encounter) -> Result<Vec<Seat>, ScanError> {
    tokio::task::spawn_blocking(move || {
        parser::interface::match_manager::get_seats_for_encounter(encounter)
    })
    .await
    .map_err(|_| ScanError::Fetch("seat fetch task panicked or was cancelled".to_string()))?
}
