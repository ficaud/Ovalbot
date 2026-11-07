//! Purchase matching: pair the seats currently offered on a match's resale
//! with the pending **buying** requests (`kind IN ('single', 'group')`) and
//! build the checkout plans the orchestrator (Phase 4B, next step) executes
//! with the user's stored shop credentials.
//!
//! **Progressive fill** (user-validated design): a request is a quota, not a
//! one-shot basket. The bot takes unclaimed **packs** as they appear — one or
//! several per pass, whatever kinds/categories they come in — until the
//! ordered count is reached. A resale pack is **atomic** (the billetterie
//! cannot split a 2-ticket listing), so the pack is the planning and checkout
//! unit: a pack is eligible only when every ticket of it matches the request
//! and the whole pack fits the remaining quota.
//! - `single`: any category, every ticket within the per-ticket budget, quota
//!   `quantity` spread over as many waves as needed;
//! - `group`: same fill logic, restricted to packs sold at the **Pelouse
//!   rate** (`15.00 €` per ticket), quota `places`. The price is the
//!   discriminator: the site's category labels are price categories
//!   (`CATÉGORIE 8` on the 2026-11-07 Bayonne resale), not the physical stand.
//!
//! Claimed seats live in `ticket_offers.claimed_by` (set by the orchestrator
//! once the add-to-cart succeeded). The matcher plans only unclaimed packs —
//! one claimed ticket takes its whole pack out of the market — and a request's
//! headroom is its quota minus its own claims, so a served pack is never
//! planned a second time, whatever the wave.
//!
//! Serving rules:
//! - Only `pending` rows in a buying kind are served; `notify` subscriptions
//!   go to the seats alerts ([`crate::services::notifications`]), `duo` is
//!   out of the Phase 4B scope (v1).
//! - **Freshness first**: the seats `record_offers` just inserted are served
//!   before older listing entries (an older pack may already be gone), and the
//!   pool is ordered deterministically (fresh, then price, then ticket id) so
//!   two identical listings always produce the same plan.
//! - Requests are served **oldest first** (`created_at ASC`): a contested
//!   seat goes to the oldest demand.
//! - A request without stored shop credentials cannot be executed: it is
//!   skipped for the pass (logged) and stays `pending`.
//!
//! Matching decisions are logged at debug (why a request was not served) and
//! info (what was planned) — the diagnostic trail for a run that bought
//! nothing must be readable straight off `RUST_LOG=ovalbot=debug`.

use std::collections::HashSet;

use parser::core::seat::Seat;
use sqlx::SqlitePool;

use crate::common::error::AppError;
use crate::services::tickets::{OfferKey, TicketKind};

// ============================== TYPES ==============================

/// One pending buying request joined with its owner's shop email and its
/// claim progress, as fetched by the internal candidate query.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PurchaseCandidate {
    /// User who made the request (`users.id`).
    pub user_id: i64,
    /// Shop email the checkout will log in with; `None` = no credentials
    /// stored, the request cannot be served yet.
    pub shop_email: Option<String>,
    /// Buying mode (`Single` or `Group` only — the SQL filters the rest).
    pub kind: TicketKind,
    /// `Single` only: how many individual tickets are wanted in total.
    pub quantity: Option<u8>,
    /// `Single` only: per-ticket budget in cents.
    pub max_price_cents: Option<u32>,
    /// `Group` only: how many pelouse places in total.
    pub places: Option<u8>,
    /// RFC 3339 insert timestamp; requests are served oldest first.
    pub created_at: String,
    /// Seats of this match this user already got into their cart (the
    /// progressive-fill progress; `0` on a fresh request).
    pub claimed: i64,
}

/// One served request: the seats the orchestrator should add to this user's
/// shop cart. The seat list is fully owned (cloned from the parsed listing).
#[derive(Debug, Clone)]
pub struct PlanCheckout {
    /// User whose credentials and cart the checkout will use.
    pub user_id: i64,
    /// LNR match id the request and the seats belong to.
    pub match_id: i64,
    /// The served buying mode (drives the add-to-cart sequence shape).
    pub kind: TicketKind,
    /// The ordered total (`quantity` for Single, `places` for Group) — the
    /// checkout reports progress as `claims/total` and closes the request when
    /// the total is reached.
    pub total: u32,
    /// The seats to add, in listing order.
    pub seats: Vec<Seat>,
}

// ============================== SERVICE ==============================

/// Plans the checkouts for one match's current resale listing, without a
/// freshness hint (every seat is treated as equally old).
///
/// Thin wrapper over [`plan_checkouts_with_fresh`] for callers that only have
/// the listing.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn plan_checkouts(
    pool: &SqlitePool,
    match_id: i64,
    seats: &[Seat],
) -> Result<Vec<PlanCheckout>, AppError> {
    plan_checkouts_with_fresh(pool, match_id, seats, &[]).await
}

/// Plans the checkouts for one match's current resale listing against its
/// pending buying requests.
///
/// Trigger contract: the scan loop calls this when `record_offers` reports
/// genuinely new seats (the durable anti-spam guard). Availability is the
/// whole listing minus the seats already claimed into a user's cart
/// (`ticket_offers.claimed_by`); plans fill a request's quota in one or many
/// waves ([`claim_seat`] records each success).
///
/// # Arguments
/// * `fresh` - The seats inserted by the current wave (`record_offers`); they
///   are served first, since an older listing entry may already be gone.
///
/// # Returns
/// One [`PlanCheckout`] per served request (oldest first), plus a debug/info
/// log trail for every decision.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn plan_checkouts_with_fresh(
    pool: &SqlitePool,
    match_id: i64,
    seats: &[Seat],
    fresh: &[OfferKey],
) -> Result<Vec<PlanCheckout>, AppError> {
    let candidates = candidate_requests(pool, match_id).await?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    // A pack is atomic: one ticket already claimed takes the whole pack out of
    // the market (the site either keeps or drops the pack as a whole).
    let claimed: HashSet<(u64, String)> = claimed_seats(pool, match_id).await?;
    let mut claimed_packs: HashSet<u64> = HashSet::new();
    for seat in seats {
        if claimed.contains(&(seat.id.pack_id, seat.id.ticket_id.clone())) {
            claimed_packs.insert(seat.id.pack_id);
        }
    }
    let mut available: Vec<&Seat> = seats
        .iter()
        .filter(|seat| !claimed_packs.contains(&seat.id.pack_id))
        .collect();
    let fresh_keys: HashSet<(u64, String)> = fresh
        .iter()
        .map(|key| (key.pack_id, key.ticket_id.clone()))
        .collect();
    sort_available(&mut available, &fresh_keys);
    let mut plans = Vec::new();

    for candidate in candidates {
        let (planned, reason, kind) = match candidate.kind {
            TicketKind::Single => {
                let (taken, reason) = plan_single(match_id, &candidate, &mut available);
                (taken, reason, TicketKind::Single)
            }
            TicketKind::Group => {
                let (taken, reason) = plan_group(match_id, &candidate, &mut available);
                (taken, reason, TicketKind::Group)
            }
            // The SQL only returns buying kinds; other kinds are logged and
            // skipped rather than trusted.
            _ => {
                tracing::debug!(
                    user_id = candidate.user_id,
                    match_id,
                    kind = candidate.kind.as_str(),
                    "purchase matching: request skipped (kind not in the buying scope)"
                );
                continue;
            }
        };
        if let Some(seats_taken) = planned {
            let mut packs: Vec<u64> = seats_taken.iter().map(|seat| seat.id.pack_id).collect();
            packs.sort_unstable();
            packs.dedup();
            tracing::info!(
                user_id = candidate.user_id,
                match_id,
                kind = kind.as_str(),
                taken = seats_taken.len(),
                claimed = candidate.claimed,
                packs = ?packs,
                "purchase matching: checkout plan"
            );
            let total = match kind {
                TicketKind::Single => u32::from(candidate.quantity.unwrap_or(0)),
                _ => u32::from(candidate.places.unwrap_or(0)),
            };
            plans.push(PlanCheckout {
                user_id: candidate.user_id,
                match_id,
                kind,
                total,
                seats: seats_taken,
            });
        } else {
            // The reason was built by the planner itself.
            tracing::debug!(
                user_id = candidate.user_id,
                match_id,
                kind = kind.as_str(),
                "purchase matching: request skipped ({}{})",
                reason,
                progress_suffix(&candidate)
            );
        }
    }

    Ok(plans)
}

/// Appends the claim progress (`2/4 filled`) to a skip reason when the
/// request has total-progress data.
fn progress_suffix(candidate: &PurchaseCandidate) -> String {
    match (candidate.kind, candidate.quantity, candidate.places) {
        (TicketKind::Single, Some(quantity), _) => {
            format!(
                "; {}/{} filled",
                candidate.claimed.min(i64::from(quantity)),
                quantity
            )
        }
        (TicketKind::Group, _, Some(places)) => format!(
            "; {}/{} filled",
            candidate.claimed.min(i64::from(places)),
            places
        ),
        _ => String::new(),
    }
}

// ============================== PLANNERS ==============================

/// Orders the availability pool the matcher scans: seats from the current wave
/// first (they are the ones still likely on sale), then by price, then by
/// ticket id — a deterministic order, since the parser yields seats from a
/// hash map and listing order used to be arbitrary.
///
/// # Arguments
/// * `available` - The unclaimed seats of the listing
/// * `fresh` - The seats inserted by the current wave
fn sort_available(available: &mut [&Seat], fresh: &HashSet<(u64, String)>) {
    available.sort_by(|left, right| {
        let left_fresh = fresh.contains(&(left.id.pack_id, left.id.ticket_id.clone()));
        let right_fresh = fresh.contains(&(right.id.pack_id, right.id.ticket_id.clone()));
        right_fresh
            .cmp(&left_fresh)
            .then_with(|| {
                let left_price = parse_price_cents(&left.price).unwrap_or(u32::MAX);
                let right_price = parse_price_cents(&right.price).unwrap_or(u32::MAX);
                left_price.cmp(&right_price)
            })
            .then_with(|| left.id.ticket_id.cmp(&right.id.ticket_id))
            .then_with(|| left.id.pack_id.cmp(&right.id.pack_id))
    });
}

/// One resale pack inside the availability pool: the atomic purchase unit.
struct PackRef {
    /// Pack identity on the site.
    pack_id: u64,
    /// Indices into the pool, in listing order.
    indices: Vec<usize>,
}

/// Groups the availability pool into its atomic packs, in first-seen order.
///
/// # Arguments
/// * `available` — the unclaimed seats of the listing
/// # Returns
/// One [`PackRef`] per pack, its indices in listing order
fn packs_of(available: &[&Seat]) -> Vec<PackRef> {
    let mut packs: Vec<PackRef> = Vec::new();
    for (index, seat) in available.iter().enumerate() {
        match packs
            .iter_mut()
            .find(|pack| pack.pack_id == seat.id.pack_id)
        {
            Some(pack) => pack.indices.push(index),
            None => packs.push(PackRef {
                pack_id: seat.id.pack_id,
                indices: vec![index],
            }),
        }
    }
    packs
}

/// Serves a `single` request: whole packs whose every ticket is within the
/// per-ticket budget, up to the remaining quota (`quantity` minus what the
/// user already claimed). A pack larger than the remaining quota is skipped
/// (the billetterie cannot split it).
fn plan_single(
    match_id: i64,
    candidate: &PurchaseCandidate,
    available: &mut Vec<&Seat>,
) -> (Option<Vec<Seat>>, String) {
    let quantity = candidate.quantity.map(u32::from).unwrap_or(0);
    let Some(max) = candidate.max_price_cents else {
        return (
            None,
            "malformed single payload (quantity or budget missing)".to_string(),
        );
    };
    if quantity == 0 {
        return (
            None,
            "malformed single payload (quantity or budget missing)".to_string(),
        );
    }
    let remaining = compute_remaining(quantity, candidate.claimed, match_id, candidate.user_id);
    if remaining == 0 {
        return (
            None,
            "order already complete, waiting for the orchestrator's status update".to_string(),
        );
    }
    if candidate.shop_email.is_none() {
        return (None, "no shop credentials stored".to_string());
    }

    // Whole packs only, capped at the remaining quota; then evict the picks
    // from the shared pool (descending, so indices stay valid). `remove` keeps
    // the pool ordered for the next request.
    let quota = remaining as usize;
    let mut take: Vec<usize> = Vec::new();
    let mut above_max_packs = 0u32;
    let mut oversized_packs = 0u32;
    for pack in packs_of(available) {
        if take.len() >= quota {
            break;
        }
        if pack.indices.len() > quota - take.len() {
            oversized_packs += 1;
            continue;
        }
        let mut eligible = true;
        for &index in &pack.indices {
            match parse_price_cents(&available[index].price) {
                Some(cents) if cents <= max => {}
                Some(_) => {
                    eligible = false;
                    above_max_packs += 1;
                    break;
                }
                None => {
                    eligible = false;
                    tracing::debug!(
                        user_id = candidate.user_id,
                        match_id,
                        pack_id = pack.pack_id,
                        price = %available[index].price,
                        "purchase matching: seat price not parsable, pack skipped"
                    );
                    break;
                }
            }
        }
        if eligible {
            take.extend(pack.indices.iter().copied());
        }
    }
    if take.is_empty() {
        let cheapest = available
            .iter()
            .filter_map(|s| parse_price_cents(&s.price))
            .min()
            .unwrap_or(max);
        return (
            None,
            format!(
                "no unclaimed pack within budget (max {}, {} pack(s) priced above it, \
                 {} pack(s) larger than the remaining {} seat(s), cheapest on resale {})",
                format_cents(max),
                above_max_packs,
                oversized_packs,
                remaining,
                format_cents(cheapest)
            ),
        );
    }

    take.sort_unstable_by(|a, b| b.cmp(a));
    let taken = take
        .iter()
        .map(|&index| (*available[index]).clone())
        .collect();
    for &index in &take {
        available.remove(index);
    }
    (Some(taken), String::new())
}

/// Serves a `group` request: packs sold at the **Pelouse rate** only, up to
/// the remaining quota (`places` minus what the user already claimed). A
/// pelouse place is identified solely by its unit price (`15.00 €`), not by
/// the category label. A pack larger than the remaining quota is skipped
/// (the billetterie cannot split it).
fn plan_group(
    match_id: i64,
    candidate: &PurchaseCandidate,
    available: &mut Vec<&Seat>,
) -> (Option<Vec<Seat>>, String) {
    let places = candidate.places.map(u32::from).unwrap_or(0);
    if places < 2 {
        return (None, "malformed group payload (places missing)".to_string());
    }
    let remaining = compute_remaining(places, candidate.claimed, match_id, candidate.user_id);
    if remaining == 0 {
        return (
            None,
            "order already complete, waiting for the orchestrator's status update".to_string(),
        );
    }
    if candidate.shop_email.is_none() {
        return (None, "no shop credentials stored".to_string());
    }

    // Whole pelouse-priced packs only, capped at the remaining quota;
    // eviction keeps the pool ordered for the next request.
    let quota = remaining as usize;
    let mut take: Vec<usize> = Vec::new();
    let mut oversized_packs = 0u32;
    let mut other_price_packs = 0u32;
    for pack in packs_of(available) {
        if take.len() >= quota {
            break;
        }
        if pack.indices.len() > quota - take.len() {
            oversized_packs += 1;
            continue;
        }
        if pack
            .indices
            .iter()
            .all(|&index| is_pelouse(available[index]))
        {
            take.extend(pack.indices.iter().copied());
        } else {
            other_price_packs += 1;
        }
    }
    if take.is_empty() {
        let mut reason = format!(
            "no unclaimed pack at the Pelouse rate ({}; need {remaining} more)",
            format_cents(PELOUSE_PRICE_CENTS)
        );
        if oversized_packs > 0 {
            reason.push_str(&format!(
                ", {oversized_packs} pack(s) larger than the remaining quota"
            ));
        }
        if other_price_packs > 0 {
            reason.push_str(&format!(", {other_price_packs} pack(s) at another price"));
        }
        return (None, reason);
    }

    take.sort_unstable_by(|a, b| b.cmp(a));
    let taken = take
        .iter()
        .map(|&index| (*available[index]).clone())
        .collect();
    for &index in &take {
        available.remove(index);
    }
    (Some(taken), String::new())
}

/// The user's remaining quota for this pass (never negative: claims cannot
/// exceed the ordered count, but a stale DB state stays defensive).
fn compute_remaining(quota: u32, claimed: i64, match_id: i64, user_id: i64) -> u32 {
    let remaining = quota.saturating_sub(claimed.max(0) as u32);
    if claimed > i64::from(quota) {
        // Over-claim (should not happen): keep serving nothing and surface it.
        tracing::warn!(
            user_id,
            match_id,
            claimed,
            "purchase matching: claims exceed the ordered quota"
        );
    }
    remaining
}

// ============================== CLAIMS ==============================

/// Counts how many of the match's seats a user already got into their cart —
/// the checkout's completion check (`claims >= total` closes the request).
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn claimed_count(
    pool: &SqlitePool,
    match_id: i64,
    user_id: i64,
) -> Result<i64, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ticket_offers WHERE match_id = ? AND claimed_by = ?",
    )
    .bind(match_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// Marks a seat as claimed into a user's cart — the orchestrator calls this
/// right after a successful add-to-cart.
///
/// Atomically wins-loses against a concurrent claim: the call returning
/// `false` means another user's cart already holds the seat between the plan
/// and the checkout (the site arbitrates the same way: a later add fails).
///
/// # Returns
/// `true` when this call won the claim, `false` when the seat was already
/// claimed (by anyone, including a repeated call) or was never recorded in
/// `ticket_offers`.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn claim_seat(
    pool: &SqlitePool,
    match_id: i64,
    pack_id: u64,
    ticket_id: &str,
    user_id: i64,
) -> Result<bool, AppError> {
    let won = sqlx::query_scalar::<_, i64>(
        "UPDATE ticket_offers SET claimed_by = ? \
         WHERE match_id = ? AND pack_id = ? AND ticket_id = ? AND claimed_by IS NULL \
         RETURNING claimed_by",
    )
    .bind(user_id)
    .bind(match_id)
    .bind(pack_id as i64)
    .bind(ticket_id)
    .fetch_optional(pool)
    .await?;
    Ok(won.is_some())
}

// ============================== SQL ==============================

/// Fetches the match's pending buying requests, oldest first, joining the
/// owner's shop email and the per-request claim count.
///
/// The kind filter is SQL-side (`kind IN ('single', 'group')`): the alert
/// subscriptions (`notify`) and the out-of-scope `duo` mode never reach the
/// matcher.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
async fn candidate_requests(
    pool: &SqlitePool,
    match_id: i64,
) -> Result<Vec<PurchaseCandidate>, AppError> {
    let rows = sqlx::query_as::<_, PurchaseCandidate>(
        "SELECT tr.user_id, u.shop_email, tr.kind, tr.quantity, tr.max_price_cents, \
             tr.places, tr.created_at, \
             (SELECT COUNT(*) FROM ticket_offers t \
              WHERE t.match_id = tr.match_id AND t.claimed_by = tr.user_id) AS claimed \
         FROM ticket_requests tr \
         JOIN users u ON u.id = tr.user_id \
         WHERE tr.match_id = ? AND tr.status = 'pending' \
               AND tr.kind IN ('single', 'group') \
         ORDER BY tr.created_at ASC, tr.user_id ASC",
    )
    .bind(match_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// The `(pack_id, ticket_id)` pairs of the match's seats already claimed into
/// someone's cart.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
async fn claimed_seats(
    pool: &SqlitePool,
    match_id: i64,
) -> Result<HashSet<(u64, String)>, AppError> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT pack_id, ticket_id FROM ticket_offers \
         WHERE match_id = ? AND claimed_by IS NOT NULL",
    )
    .bind(match_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(pack_id, ticket_id)| (pack_id as u64, ticket_id))
        .collect())
}

// ============================== HELPERS ==============================

/// Unit price of a Pelouse place, in cents: the site's flat pelouse rate
/// (`15.00 €`). Group requests are matched on this exact price.
const PELOUSE_PRICE_CENTS: u32 = 1_500;

/// Whether the seat is a Pelouse place, identified **solely by its unit
/// price** (`15.00 €`). The category/tribune labels are not used: `category`
/// is a price category (`CATÉGORIE 8` on the live 2026-11-07 Bayonne page),
/// and the physical "Tribune PELOUSE" text is not a stable classifier.
fn is_pelouse(seat: &Seat) -> bool {
    parse_price_cents(&seat.price) == Some(PELOUSE_PRICE_CENTS)
}

/// Renders whole cents as a display price (`4900` → `"49.00 €"`).
fn format_cents(cents: u32) -> String {
    format!("{:.2} €", cents as f64 / 100.0)
}

// ============================== PRICE PARSING ==============================

/// Parses a resale price string into whole cents.
///
/// Tolerates the shapes the site prints (`"49.00"`, `"59"`, `"1 240,50 €"`,
/// spaces / non-breaking spaces in thousands separators): digits plus one
/// decimal separator; the separator is the **last** `,`/`.` of the cleaned
/// string (everything before it regroups as the integer part). A price that
/// parses to `0` cents is treated as un-parsable (a free seat is a page
/// anomaly, not a real offer).
///
/// # Returns
/// Price in cents, or `None` when no price can be extracted.
pub fn parse_price_cents(raw: &str) -> Option<u32> {
    // Commas become decimal dots; everything else non-numeric disappears.
    let digits_sep: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == ',' || *c == '.')
        .map(|c| if c == ',' { '.' } else { c })
        .collect();
    let (int_part, frac_part) = match digits_sep.rsplit_once('.') {
        Some((int_part, frac)) => (int_part, frac.chars().take(2).collect::<String>()),
        None => (digits_sep.as_str(), String::new()),
    };
    let int_digits: String = int_part.chars().filter(|c| c.is_ascii_digit()).collect();
    if int_digits.is_empty() && frac_part.is_empty() {
        return None;
    }
    let whole: u32 = int_digits.parse().unwrap_or(0);
    let frac_padded = format!("{frac_part:0<2}");
    let cents = whole
        .checked_mul(100)?
        .checked_add(frac_padded.parse::<u32>().ok()?)?;
    (cents > 0).then_some(cents)
}

// ============================== TESTS ==============================

#[cfg(test)]
mod tests {
    use super::*;
    use parser::core::seat::{SeatComposition, SeatId, SeatInfo};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Monotonic pack ids so each [`seat`] call mints its own 1-ticket pack.
    static NEXT_PACK: AtomicU64 = AtomicU64::new(1);

    /// Builds a resale seat in its own 1-ticket pack (resale packs are atomic,
    /// so independent test seats must not share a pack).
    fn seat(price: &str, category: &str) -> Seat {
        let pack_id = NEXT_PACK.fetch_add(1, Ordering::Relaxed);
        seat_in(pack_id, &format!("t{pack_id}"), price, category)
    }

    /// Builds a resale seat inside an explicit pack (pack id + ticket id).
    fn seat_in(pack_id: u64, ticket_id: &str, price: &str, category: &str) -> Seat {
        Seat {
            id: SeatId {
                pack_id,
                ticket_id: ticket_id.to_string(),
            },
            seat_info: SeatInfo {
                full_name: format!("{category} Place 12"),
                composition: SeatComposition {
                    category: category.to_string(),
                    bloc: "NORD".to_string(),
                    row: "U".to_string(),
                    seat_number: 12,
                },
            },
            price: price.to_string(),
        }
    }

    fn single_candidate(
        quantity: u8,
        max_price_cents: u32,
        credentials: bool,
        claimed: i64,
    ) -> PurchaseCandidate {
        PurchaseCandidate {
            user_id: 1,
            shop_email: credentials.then(|| "shop@example.com".to_string()),
            kind: TicketKind::Single,
            quantity: Some(quantity),
            max_price_cents: Some(max_price_cents),
            places: None,
            created_at: String::new(),
            claimed,
        }
    }

    fn group_candidate(places: u8, credentials: bool, claimed: i64) -> PurchaseCandidate {
        PurchaseCandidate {
            user_id: 1,
            shop_email: credentials.then(|| "shop@example.com".to_string()),
            kind: TicketKind::Group,
            quantity: None,
            max_price_cents: None,
            places: Some(places),
            created_at: String::new(),
            claimed,
        }
    }

    // The matcher order is freshness first, then price, then ticket id: a
    // current-wave seat is never shadowed by an older listing entry.
    #[test]
    fn sort_available_puts_fresh_seats_first() {
        let old_cheap = seat("39.00", "CATÉGORIE 3");
        let fresh_pricey = seat("49.00", "CATÉGORIE 3");
        let fresh_cheap = seat("45.00", "CATÉGORIE 3");
        let mut available = vec![&old_cheap, &fresh_pricey, &fresh_cheap];
        let fresh: HashSet<(u64, String)> = [&fresh_pricey, &fresh_cheap]
            .iter()
            .map(|seat| (seat.id.pack_id, seat.id.ticket_id.clone()))
            .collect();

        sort_available(&mut available, &fresh);

        let ids: Vec<&str> = available
            .iter()
            .map(|seat| seat.id.ticket_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec![
                fresh_cheap.id.ticket_id.as_str(),
                fresh_pricey.id.ticket_id.as_str(),
                old_cheap.id.ticket_id.as_str()
            ]
        );
    }

    // Without a freshness hint the order is still deterministic: cheapest
    // first, then ticket id.
    #[test]
    fn sort_available_is_deterministic() {
        let cheap = seat("40.00", "CATÉGORIE 3");
        let pricey = seat("49.00", "CATÉGORIE 3");
        let mut forward = vec![&cheap, &pricey];
        let mut backward = vec![&pricey, &cheap];

        sort_available(&mut forward, &HashSet::new());
        sort_available(&mut backward, &HashSet::new());

        assert_eq!(forward[0].id.ticket_id, cheap.id.ticket_id);
        assert_eq!(backward[0].id.ticket_id, cheap.id.ticket_id);
    }

    // Common integer / decimal price shapes all land on the expected cents.
    #[test]
    fn parses_the_sites_price_shapes() {
        assert_eq!(parse_price_cents("49.00"), Some(4900));
        assert_eq!(parse_price_cents("49"), Some(4900));
        assert_eq!(parse_price_cents("59,50"), Some(5950));
        assert_eq!(parse_price_cents("49.00 €"), Some(4900));
        assert_eq!(parse_price_cents("1 240,50 €"), Some(124_050));
    }

    // Junk / zero prices are rejected (parse failure, not "free seat").
    #[test]
    fn rejects_unparsable_prices() {
        for bad in ["", "gratuit", "0", "0.00", "€"] {
            assert_eq!(parse_price_cents(bad), None, "{bad:?} should not parse");
        }
    }

    // Pelouse detection is price-based: only the 15.00 € unit price matters,
    // whatever the category/tribune labels say.
    #[test]
    fn pelouse_detection_is_price_based() {
        // The live 2026-11-07 shape: physically PELOUSE, labelled CATÉGORIE 8.
        assert!(is_pelouse(&seat("15.00", "CATÉGORIE 8")));
        assert!(is_pelouse(&seat("15", "PELOUSE")));
        assert!(is_pelouse(&seat("15,00", "")));
        assert!(!is_pelouse(&seat("14.99", "PELOUSE")));
        assert!(!is_pelouse(&seat("16.00", "PELOUSE")));
        assert!(!is_pelouse(&seat("33.00", "PELOUSE")));
        assert!(!is_pelouse(&seat("", "PELOUSE")));
    }

    // The log price renderer keeps two decimals.
    #[test]
    fn format_cents_renders_two_decimals() {
        assert_eq!(format_cents(4900), "49.00 €");
        assert_eq!(format_cents(124_050), "1240.50 €");
    }

    // A single request takes what's available, capped at the remaining quota.
    #[test]
    fn plan_single_caps_at_the_remaining_quota() {
        let s1 = seat("40.00", "CATÉGORIE 3");
        let s2 = seat("45.00", "CATÉGORIE 2");
        let mut available = vec![&s1, &s2];

        // Quota 4, already claimed 3 → only 1 more seat is taken.
        let (taken, reason) = plan_single(42, &single_candidate(4, 5000, true, 3), &mut available);
        assert!(reason.is_empty(), "{reason}");
        assert_eq!(taken.unwrap().len(), 1);
        assert_eq!(available.len(), 1);

        // The fully-claimed variant refuses instead.
        let (taken, reason) = plan_single(42, &single_candidate(4, 5000, true, 4), &mut available);
        assert!(taken.is_none());
        assert!(reason.contains("order already complete"), "{reason}");
    }

    // A group request works like Single: whatever pelouse is available, up
    // to the remaining quota — no "N at once" gate anymore.
    #[test]
    fn plan_group_takes_the_pelouse_available() {
        let candidate = group_candidate(4, true, 0);

        // One pelouse (live CATÉGORIE 8 label) for a 4-place request → 1 planned.
        let only = seat("15.00", "CATÉGORIE 8");
        let mut one_pelouse = vec![&only];
        let (taken, reason) = plan_group(42, &candidate, &mut one_pelouse);
        assert!(reason.is_empty(), "{reason}");
        assert_eq!(taken.unwrap().len(), 1);

        // Two pelouse-priced seats among a cat seat → those two planned.
        let pelouse1 = seat("15.00", "CATÉGORIE 8");
        let pelouse2 = seat("15.00", "PELOUSE");
        let cat = seat("45.00", "CATÉGORIE 2");
        let mut listing = vec![&pelouse1, &cat, &pelouse2];
        let (taken, reason) = plan_group(42, &candidate, &mut listing);
        assert!(reason.is_empty(), "{reason}");
        let taken = taken.unwrap();
        assert_eq!(taken.len(), 2);
        assert!(taken.iter().all(is_pelouse));

        // Remaining quota respected: 2/4 filled, 1 pelouse left → 1 seat.
        let mut late = vec![&pelouse1];
        let (taken, reason) = plan_group(42, &group_candidate(4, true, 2), &mut late);
        assert!(reason.is_empty(), "{reason}");
        assert_eq!(taken.unwrap().len(), 1);
    }

    // A pack is atomic: a 2-ticket pack cannot serve a 1-seat quota.
    #[test]
    fn plan_single_skips_a_pack_larger_than_the_quota() {
        let duo_a = seat_in(900, "a", "40.00", "CATÉGORIE 3");
        let duo_b = seat_in(900, "b", "40.00", "CATÉGORIE 3");
        let mut available = vec![&duo_a, &duo_b];
        let (taken, reason) = plan_single(42, &single_candidate(1, 5000, true, 0), &mut available);
        assert!(taken.is_none());
        assert!(
            reason.contains("1 pack(s) larger than the remaining 1 seat(s)"),
            "{reason}"
        );
        assert_eq!(available.len(), 2, "the pack stays on the market");
    }

    // When the quota covers the whole pack, every ticket of it is planned at
    // once (the checkout will send one add-to-cart POST for the pack).
    #[test]
    fn plan_single_takes_a_whole_pack_within_the_quota() {
        let duo_a = seat_in(901, "a", "40.00", "CATÉGORIE 3");
        let duo_b = seat_in(901, "b", "40.00", "CATÉGORIE 3");
        let mut available = vec![&duo_a, &duo_b];
        let (taken, reason) = plan_single(42, &single_candidate(2, 5000, true, 0), &mut available);
        assert!(reason.is_empty(), "{reason}");
        let taken = taken.unwrap();
        assert_eq!(taken.len(), 2);
        assert!(taken.iter().all(|s| s.id.pack_id == 901));
        assert!(available.is_empty());
    }

    // A Pelouse duo is one purchase unit: a 4-place group takes both tickets.
    #[test]
    fn plan_group_takes_a_whole_pelouse_pack() {
        let duo_a = seat_in(902, "a", "15.00", "Pelouse");
        let duo_b = seat_in(902, "b", "15.00", "Pelouse");
        let mut available = vec![&duo_a, &duo_b];
        let (taken, reason) = plan_group(42, &group_candidate(4, true, 0), &mut available);
        assert!(reason.is_empty(), "{reason}");
        let taken = taken.unwrap();
        assert_eq!(taken.len(), 2);
        assert!(taken.iter().all(|s| s.id.pack_id == 902));
    }

    // A pelouse pack bigger than the remaining quota is skipped, not split.
    #[test]
    fn plan_group_skips_a_pack_that_does_not_fit() {
        let trio = [
            seat_in(903, "a", "15.00", "Pelouse"),
            seat_in(903, "b", "15.00", "Pelouse"),
            seat_in(903, "c", "15.00", "Pelouse"),
        ];
        let mut available: Vec<&Seat> = trio.iter().collect();
        // 3/4 filled: 1 place left, a 3-ticket pack does not fit.
        let (taken, reason) = plan_group(42, &group_candidate(4, true, 3), &mut available);
        assert!(taken.is_none());
        assert!(
            reason.contains("1 pack(s) larger than the remaining quota"),
            "{reason}"
        );
    }

    // A mixed-price pack cannot serve a group: the billetterie sells it whole,
    // and not every ticket is at the pelouse rate.
    #[test]
    fn plan_group_ignores_a_mixed_pack() {
        let pelouse = seat_in(904, "a", "15.00", "CATÉGORIE 8");
        let cat = seat_in(904, "b", "45.00", "CATÉGORIE 2");
        let mut available = vec![&pelouse, &cat];
        let (taken, reason) = plan_group(42, &group_candidate(4, true, 0), &mut available);
        assert!(taken.is_none());
        assert!(
            reason.contains("no unclaimed pack at the Pelouse rate"),
            "{reason}"
        );
    }

    // The price is the only pelouse criterion: a seat labelled PELOUSE at
    // another price is not a pelouse place.
    #[test]
    fn plan_group_ignores_a_pelouse_labelled_pack_at_another_price() {
        let mislabelled = seat("49.00", "PELOUSE");
        let mut available = vec![&mislabelled];
        let (taken, reason) = plan_group(42, &group_candidate(4, true, 0), &mut available);
        assert!(taken.is_none());
        assert!(
            reason.contains("no unclaimed pack at the Pelouse rate"),
            "{reason}"
        );
        assert!(
            reason.contains("1 pack(s) at another price"),
            "the diagnostic must name the rejected pack: {reason}"
        );
    }

    // A request without shop credentials is skipped, whatever the seats.
    #[test]
    fn candidates_without_credentials_are_skipped() {
        let s1 = seat("40.00", "CATÉGORIE 3");
        let pelouse = seat("15.00", "Pelouse");
        let mut available = vec![&s1, &pelouse];

        let (taken, reason) = plan_single(42, &single_candidate(2, 5000, false, 0), &mut available);
        assert!(taken.is_none());
        assert_eq!(reason, "no shop credentials stored");

        let (taken, reason) = plan_group(42, &group_candidate(2, false, 0), &mut available);
        assert!(taken.is_none());
        assert_eq!(reason, "no shop credentials stored");
    }

    // The budget filter only keeps seats at or under the per-ticket max.
    #[test]
    fn plan_single_respects_the_budget() {
        let seat_over = seat("55.00", "CATÉGORIE 1");
        let seat_under = seat("45.00", "CATÉGORIE 3");
        let mut available = vec![&seat_over, &seat_under];
        let (taken, reason) = plan_single(42, &single_candidate(2, 4900, true, 0), &mut available);
        assert!(reason.is_empty(), "{reason}");
        let taken = taken.unwrap();
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].price, "45.00");
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].price, "55.00");
    }

    // An unmatched budget still produces a priced diagnostic reason.
    #[test]
    fn plan_single_reports_why_the_budget_failed() {
        let over = seat("55.00", "CATÉGORIE 1");
        let over_too = seat("60.00", "CATÉGORIE 1");
        let mut available = vec![&over, &over_too];
        let (taken, reason) = plan_single(42, &single_candidate(2, 4900, true, 0), &mut available);
        assert!(taken.is_none());
        assert!(
            reason.contains("no unclaimed pack within budget"),
            "{reason}"
        );
        assert!(reason.contains("max 49.00 €"), "{reason}");
        assert!(reason.contains("cheapest on resale 55.00 €"), "{reason}");
        assert!(reason.contains("2 pack(s)"), "{reason}");
    }

    // An empty listing produces the same priced diagnostic structure.
    #[test]
    fn plan_single_with_no_listing_reports_naturally() {
        let mut available: Vec<&Seat> = Vec::new();
        let (taken, reason) = plan_single(42, &single_candidate(1, 4900, true, 0), &mut available);
        assert!(taken.is_none());
        assert!(
            reason.contains("no unclaimed pack within budget"),
            "{reason}"
        );
    }

    // A depleted group quota logs a "need more" reason mentioning the count.
    #[test]
    fn plan_group_reports_the_missing_pelouse() {
        let cat = seat("45.00", "CATÉGORIE 2");
        let mut available = vec![&cat];
        let (taken, reason) = plan_group(42, &group_candidate(2, true, 0), &mut available);
        assert!(taken.is_none());
        assert!(
            reason.contains("no unclaimed pack at the Pelouse rate"),
            "{reason}"
        );
        assert!(reason.contains("need 2 more"), "{reason}");
    }

    // Evicting several seats keeps later planners from double-planning them:
    // two quantity-1 requests share a 2-seat listing exactly once each, and
    // the second pass over a depleted pool plans nothing.
    #[test]
    fn consecutive_plans_do_not_share_seats() {
        let mut s1 = seat("40.00", "CATÉGORIE 3");
        let mut s2 = seat("45.00", "CATÉGORIE 3");
        s1.id.ticket_id = "a".to_string();
        s2.id.ticket_id = "b".to_string();
        let mut available = vec![&s1, &s2];

        let (first, _) = plan_single(42, &single_candidate(1, 5000, true, 0), &mut available);
        let (second, _) = plan_single(42, &single_candidate(1, 5000, true, 0), &mut available);
        let (third, reason) = plan_single(42, &single_candidate(1, 5000, true, 0), &mut available);

        assert!(first.is_some() && second.is_some());
        let got = [first.unwrap(), second.unwrap()]
            .iter()
            .map(|seats| seats[0].id.ticket_id.clone())
            .collect::<Vec<_>>();
        assert_ne!(
            got[0], got[1],
            "two plans must not claim the same seat: {got:?}"
        );
        assert!(third.is_none());
        assert!(
            reason.contains("no unclaimed pack within budget"),
            "{reason}"
        );
    }

    // The exact live seat shape logged on the Bayonne resale (2026-11-07):
    // physically Pelouse, but the site's price category is CATÉGORIE 8. The
    // matcher must recognize it by its 15.00 € price, not the category label.
    #[test]
    fn takes_the_live_pelouse_seat_as_a_candidate() {
        let live = Seat {
            id: SeatId {
                pack_id: 35596,
                ticket_id: "51955".to_string(),
            },
            seat_info: SeatInfo {
                full_name: "Tribune PELOUSE • Bloc PELOUSE • Accès PELOUSE • Siège 230".to_string(),
                composition: SeatComposition {
                    category: "CATÉGORIE 8".to_string(),
                    bloc: "PELOUSE".to_string(),
                    row: String::new(),
                    seat_number: 230,
                },
            },
            price: "15.00".to_string(),
        };
        assert!(is_pelouse(&live));

        let mut available = vec![&live];
        let (taken, reason) = plan_group(11877, &group_candidate(4, true, 0), &mut available);
        assert!(reason.is_empty(), "{reason}");
        let taken = taken.unwrap();
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].id.pack_id, 35596);
        assert_eq!(taken[0].id.ticket_id, "51955");

        let mut available = vec![&live];
        let (taken, reason) =
            plan_single(11877, &single_candidate(1, 1500, true, 0), &mut available);
        assert!(reason.is_empty(), "{reason}");
        assert_eq!(taken.unwrap().len(), 1);
    }
}
