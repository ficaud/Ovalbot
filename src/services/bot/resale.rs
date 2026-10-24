//! Durable registry of the billetterie resale links (`resale_links` table)
//! and the polling target list for the bot's direct scan pass.
//!
//! Flow: the bot's discovery pass parses the ticketing home page and hands
//! every card carrying an open resale link to [`upsert_discovered`]; from
//! then on, the direct poll pass reads [`links_to_scan`] and fetches those
//! pages without touching the home page again. The home page therefore only
//! has to be reachable for *discovery* — a queued home no longer stops the
//! polling of links learned earlier.
//!
//! Lifecycle: one row per (title, date) — the stable site keys. A later
//! discovery pass replaces the link when a new resale wave opens a new
//! product, bumps `last_seen_at` and refreshes the resolved `match_id`. Rows
//! are deleted by the scan loop once the match date has passed.

use parser::core::club::ClubType;
use parser::core::encounter::Encounter;
use sqlx::FromRow;
use sqlx::SqlitePool;

use crate::common::error::AppError;
use crate::common::time;

// ============================== TYPES ==============================

/// One persisted resale link to poll directly.
#[derive(Debug, Clone, FromRow)]
pub struct ScanTarget {
    /// Encounter title (stable key half).
    pub title: String,
    /// Encounter date string (stable key half).
    pub date: String,
    /// The last known resale product URL.
    pub resale_link: String,
    /// Resolved LNR match id; `None` when the card is outside the calendar
    /// (basket games, meals…): the link is still polled, offers still not
    /// recorded.
    pub match_id: Option<i64>,
}

impl ScanTarget {
    /// Rebuilds the parser `Encounter` this target came from, so the seat
    /// fetch and the `date_passed()` check can reuse the parser API.
    ///
    /// # Returns
    /// An `Encounter` with the stored title/date/link; the nature is
    /// informational only (the resolver never looks at it).
    #[must_use]
    pub fn to_encounter(&self) -> Encounter {
        Encounter::new(
            ClubType::StadeRochelais,
            self.title.clone(),
            self.date.clone(),
            parser::core::encounter::MatchNature::Other,
            Some(self.resale_link.clone()),
        )
    }
}

/// Counters of one home-discovery pass, feeding the log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveryCounts {
    /// Encounters that carried an open resale link.
    pub with_link: usize,
    /// Brand-new rows inserted by this pass.
    pub inserted: usize,
    /// Rows refreshed by this pass (link or match id changed, or just the
    /// `last_seen_at` bump).
    pub updated: usize,
}

/// Outcome of one direct poll of a resale link, stored in
/// `resale_links.last_scan_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStatus {
    /// The product page was fetched and parsed (seats present or not).
    Ok,
    /// The page fetched fine but carried no seat.
    NoSeats,
    /// The shop answered with its waiting-queue page; retry next tick.
    Queue,
    /// The fetch itself failed (timeout, DNS, dead link…); retry next tick.
    HttpError,
}

/// The column spelling of [`ScanStatus`].
impl ScanStatus {
    /// The string stored in `last_scan_status`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ScanStatus::Ok => "ok",
            ScanStatus::NoSeats => "no_seats",
            ScanStatus::Queue => "queue",
            ScanStatus::HttpError => "http_error",
        }
    }
}

// ============================== DISCOVERY UPSERT ==============================

/// Persists the resale links found by one home-discovery pass.
///
/// Only encounters whose `resale_link` is `Some` are stored — links-only
/// table, by contract. The pass must not touch rows absent from it: a link
/// learned earlier stays pollable even when the home page has stopped
/// advertising it (resale closed but product page still reachable). For each
/// listed encounter, the link (`replaced` when a new resale wave opens a new
/// product), the re-resolved `match_id` and `last_seen_at` are refreshed,
/// while `first_seen_at` and the scan bookkeeping are preserved.
///
/// # Arguments
/// * `encounters` - The encounters parsed from the home page
/// * `match_ids` - One entry per encounter, index-aligned: the resolved LNR
///   id, or `None` when the card is outside the calendar
/// # Returns
/// The pass counters for the discovery log line.
/// # Errors
/// `AppError::Sqlx` on query failure or resolution of the update.
pub async fn upsert_discovered(
    pool: &SqlitePool,
    encounters: &[Encounter],
    match_ids: &[Option<u32>],
) -> Result<DiscoveryCounts, AppError> {
    assert_eq!(
        encounters.len(),
        match_ids.len(),
        "upsert_discovered: index-aligned inputs required"
    );
    let now = time::now();
    let mut counts = DiscoveryCounts {
        with_link: 0,
        inserted: 0,
        updated: 0,
    };

    for (encounter, match_id) in encounters.iter().zip(match_ids.iter().copied()) {
        let Some(link) = encounter.resale_link.as_ref() else {
            continue;
        };
        counts.with_link += 1;
        // An INSERT fills first_seen_at with `now`; an update on conflict
        // keeps the old value — so `first_seen_at = now` tells which branch
        // the row took. (`excluded` is not usable in RETURNING.)
        let was_inserted = sqlx::query_scalar::<_, i64>(
            "INSERT INTO resale_links \
                 (title, date, resale_link, match_id, first_seen_at, last_seen_at) \
              VALUES (?, ?, ?, ?, ?, ?) \
              ON CONFLICT(title, date) DO UPDATE SET \
                  resale_link = excluded.resale_link, \
                  match_id = excluded.match_id, \
                  last_seen_at = excluded.last_seen_at \
              RETURNING (first_seen_at = ?)",
        )
        .bind(&encounter.title)
        .bind(&encounter.date)
        .bind(link)
        .bind(match_id.map(i64::from))
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .fetch_one(pool)
        .await?;

        if was_inserted == 1 {
            counts.inserted += 1;
            tracing::info!(
                title = %encounter.title,
                date = %encounter.date,
                match_id = match_id,
                "resale link discovered"
            );
        } else {
            counts.updated += 1;
        }
    }

    if counts.with_link > 0 {
        tracing::debug!(
            links = counts.with_link,
            inserted = counts.inserted,
            updated = counts.updated,
            "resale links persisted"
        );
    }
    Ok(counts)
}

// ============================== POLLING TARGETS ==============================

/// Returns every stored resale link to poll directly, in a deterministic
/// order (first seen, then title) so a scan pass walks the table the same
/// way every time.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn links_to_scan(pool: &SqlitePool) -> Result<Vec<ScanTarget>, AppError> {
    let targets = sqlx::query_as::<_, ScanTarget>(
        "SELECT title, date, resale_link, match_id \
         FROM resale_links \
         ORDER BY first_seen_at, title, date",
    )
    .fetch_all(pool)
    .await?;
    Ok(targets)
}

/// Records the outcome of one direct poll.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn mark_scanned(
    pool: &SqlitePool,
    title: &str,
    date: &str,
    status: ScanStatus,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE resale_links SET last_scanned_at = ?, last_scan_status = ? \
              WHERE title = ? AND date = ?",
    )
    .bind(time::now())
    .bind(status.as_str())
    .bind(title)
    .bind(date)
    .execute(pool)
    .await?;
    Ok(())
}

// ============================== RETIREMENT ==============================

/// Deletes the stored link of a past-dated match (e.g. a match whose date
/// has passed): the product page could be recycled by a future wave, so
/// polling it further is pointless and potentially wrong.
///
/// # Returns
/// `true` when a row was deleted.
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn delete_one(pool: &SqlitePool, title: &str, date: &str) -> Result<bool, AppError> {
    let out = sqlx::query("DELETE FROM resale_links WHERE title = ? AND date = ?")
        .bind(title)
        .bind(date)
        .execute(pool)
        .await?;
    Ok(out.rows_affected() > 0)
}
