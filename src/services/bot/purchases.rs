//! Purchase matching: pair the seats currently offered on a match's resale
//! with the pending **buying** requests (`kind IN ('single', 'group')`) and
//! build the checkout plans the orchestrator (Phase 4B, next step) executes
//! with the user's stored shop credentials.
//!
//! **Progressive fill** (user-validated design): a request is a quota, not a
//! one-shot basket. The bot takes unclaimed seats as they appear — one or
//! several per pass, whatever kinds/categories they come in — until the
//! ordered count is reached:
//! - `single`: any category, price within the per-ticket budget, quota
//!   `quantity` spread over as many waves as needed;
//! - `group`: exactly like single but restricted to **Pelouse** seats (the
//!   pelouse rate applies, no price match), quota `places`.
//!
//! Claimed seats live in `ticket_offers.claimed_by` (set by the orchestrator
//! once the add-to-cart succeeded). The matcher plans only unclaimed seats
//! and a request's headroom is its quota minus its own claims — so a served
//! seat is never planned a second time, whatever the wave.
//!
//! Serving rules:
//! - Only `pending` rows in a buying kind are served; `notify` subscriptions
//!   go to the seats alerts ([`crate::services::notifications`]), `duo` is
//!   out of the Phase 4B scope (v1).
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
use crate::services::tickets::TicketKind;

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

/// Plans the checkouts for one match's current resale listing against its
/// pending buying requests.
///
/// Trigger contract: the scan loop calls this when `record_offers` reports
/// genuinely new seats (the durable anti-spam guard). Availability is the
/// whole listing minus the seats already claimed into a user's cart
/// (`ticket_offers.claimed_by`); plans fill a request's quota in one or many
/// waves ([`claim_seat`] records each success).
///
/// # Returns
/// One [`PlanCheckout`] per served request (oldest first), plus a debug/info
/// log trail for every decision.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn plan_checkouts(
    pool: &SqlitePool,
    match_id: i64,
    seats: &[Seat],
) -> Result<Vec<PlanCheckout>, AppError> {
    let candidates = candidate_requests(pool, match_id).await?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    // Seats already in someone's cart drop out of the pool: the site keeps
    // them listed but only one user's add-to-cart can ever win them.
    let claimed: HashSet<(u64, String)> = claimed_seats(pool, match_id).await?;
    let mut available: Vec<&Seat> = seats
        .iter()
        .filter(|seat| !claimed.contains(&(seat.id.pack_id, seat.id.ticket_id.clone())))
        .collect();
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
            tracing::info!(
                user_id = candidate.user_id,
                match_id,
                kind = kind.as_str(),
                taken = seats_taken.len(),
                claimed = candidate.claimed,
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

/// Serves a `single` request: seats within the per-ticket budget, up to the
/// remaining quota (`quantity` minus what the user already claimed).
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

    // First the eligible picks (capped at the remaining quota), then evict
    // them from the shared pool — descending, because `swap_remove` shifts
    // the last element into the removed slot.
    let mut within: Vec<usize> = Vec::new();
    let mut above_max = 0u32;
    for (index, seat) in available.iter().enumerate() {
        if within.len() == remaining as usize {
            break;
        }
        match parse_price_cents(&seat.price) {
            Some(cents) if cents <= max => within.push(index),
            Some(_) => above_max += 1,
            None => tracing::debug!(
                user_id = candidate.user_id,
                match_id,
                price = %seat.price,
                "purchase matching: seat price not parsable, seat skipped"
            ),
        }
    }
    if within.is_empty() {
        let cheapest = available
            .iter()
            .filter_map(|s| parse_price_cents(&s.price))
            .min()
            .unwrap_or(max);
        return (
            None,
            format!(
                "no unclaimed seat within budget (max {}, {above_max} seat(s) priced \
                 above it, cheapest on resale {})",
                format_cents(max),
                format_cents(cheapest)
            ),
        );
    }

    let mut taken_indices = within.clone();
    taken_indices.sort_unstable_by(|a, b| b.cmp(a));
    let taken = taken_indices
        .iter()
        .map(|&index| (*available[index]).clone())
        .collect();
    for &index in &taken_indices {
        available.swap_remove(index);
    }
    (Some(taken), String::new())
}

/// Serves a `group` request: **Pelouse** seats only, up to the remaining
/// quota (`places` minus what the user already claimed). The pelouse rate
/// applies, so no price match.
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

    // First the eligible picks (capped at the remaining quota), then evict
    // them from the shared pool — descending, because `swap_remove` shifts
    // the last element into the removed slot.
    let pelouse: Vec<usize> = available
        .iter()
        .enumerate()
        .filter(|(_, seat)| is_pelouse(seat))
        .map(|(index, _)| index)
        .take(remaining as usize)
        .collect();
    if pelouse.is_empty() {
        return (
            None,
            format!(
                "no unclaimed Pelouse seat on the market (need {} more)",
                remaining
            ),
        );
    }

    let mut taken_indices = pelouse.clone();
    taken_indices.sort_unstable_by(|a, b| b.cmp(a));
    let taken = taken_indices
        .iter()
        .map(|&index| (*available[index]).clone())
        .collect();
    for &index in &taken_indices {
        available.swap_remove(index);
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

/// Whether the seat is a Pelouse place (the site's category name, compared
/// case-insensitively; the composition carries it verbatim from the page).
fn is_pelouse(seat: &Seat) -> bool {
    seat.seat_info
        .composition
        .category
        .eq_ignore_ascii_case("pelouse")
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

    /// Builds a resale seat with a category and a raw price string.
    fn seat(price: &str, category: &str) -> Seat {
        Seat {
            id: SeatId {
                pack_id: 33727,
                ticket_id: "t1".to_string(),
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

    // Pelouse detection is case-insensitive and exact-name based.
    #[test]
    fn pelouse_detection_is_case_insensitive() {
        assert!(is_pelouse(&seat("49.00", "Pelouse")));
        assert!(is_pelouse(&seat("49.00", "PELOUSE")));
        assert!(!is_pelouse(&seat("49.00", "CATÉGORIE 3")));
        assert!(!is_pelouse(&seat("49.00", "")));
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
        let s2 = seat("45.00", "Pelouse");
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

        // One pelouse for a 4-place request → 1 planned (not nothing).
        let only = seat("15.00", "Pelouse");
        let mut one_pelouse = vec![&only];
        let (taken, reason) = plan_group(42, &candidate, &mut one_pelouse);
        assert!(reason.is_empty(), "{reason}");
        assert_eq!(taken.unwrap().len(), 1);

        // Two pelouse (one case-variant) among a cat seat → those two planned.
        let pelouse1 = seat("15.00", "Pelouse");
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
            reason.contains("no unclaimed seat within budget"),
            "{reason}"
        );
        assert!(reason.contains("max 49.00 €"), "{reason}");
        assert!(reason.contains("cheapest on resale 55.00 €"), "{reason}");
        assert!(reason.contains("2 seat(s)"), "{reason}");
    }

    // An empty listing produces the same priced diagnostic structure.
    #[test]
    fn plan_single_with_no_listing_reports_naturally() {
        let mut available: Vec<&Seat> = Vec::new();
        let (taken, reason) = plan_single(42, &single_candidate(1, 4900, true, 0), &mut available);
        assert!(taken.is_none());
        assert!(
            reason.contains("no unclaimed seat within budget"),
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
        assert!(reason.contains("no unclaimed Pelouse"), "{reason}");
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
            reason.contains("no unclaimed seat within budget"),
            "{reason}"
        );
    }

    // The exact live seat shape logged on the Bayonne resale (2026-10-29):
    // upper-case category, `15.00` price, numeric ticket id. Both group and
    // single modes must take it as a candidate (group ignores the price).
    #[test]
    fn takes_the_live_pelouse_seat_as_a_candidate() {
        let live = Seat {
            id: SeatId {
                pack_id: 34677,
                ticket_id: "50583".to_string(),
            },
            seat_info: SeatInfo {
                full_name: "Tribune PELOUSE • Bloc PELOUSE • Accès PELOUSE • Siège 147".to_string(),
                composition: SeatComposition {
                    category: "PELOUSE".to_string(),
                    bloc: "PELOUSE".to_string(),
                    row: String::new(),
                    seat_number: 147,
                },
            },
            price: "15.00".to_string(),
        };

        let mut available = vec![&live];
        let (taken, reason) = plan_group(11877, &group_candidate(4, true, 0), &mut available);
        assert!(reason.is_empty(), "{reason}");
        let taken = taken.unwrap();
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].id.pack_id, 34677);
        assert_eq!(taken[0].id.ticket_id, "50583");

        let mut available = vec![&live];
        let (taken, reason) =
            plan_single(11877, &single_candidate(1, 1500, true, 0), &mut available);
        assert!(reason.is_empty(), "{reason}");
        assert_eq!(taken.unwrap().len(), 1);
    }
}
