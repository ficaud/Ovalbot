//! Checkout orchestrator: turns the matcher's [`PlanCheckout`]s into
//! real add-to-cart calls on the billetterie, in the **user's own cart**, and
//! reports each wave.
//!
//! Flow per plan (plans are executed strictly one after the other — the site
//! cannot be driven concurrently and a single shop session at a time keeps the
//! cart state coherent):
//! 1. respect `CART_ENABLED` / `CART_DRY_RUN` (dry-run = decisions logged and
//!    credential presence checked, no login, no POST, no claim, no
//!    notification);
//! 2. load the user's decrypted shop credentials ([`crate::services::accounts::shop`]);
//!    a missing key (rotated `SECRET_KEY`) or missing credentials leaves the
//!    request `pending` — nothing can be acted on;
//! 3. log in ([`CartShop::login`]) and add each planned **pack**
//!    ([`CartSession::add_pack`]) — a resale pack is atomic, so one POST per
//!    pack, carrying every ticket of it;
//! 4. a successful add is recorded with [`crate::services::bot::purchases::claim_seat`]
//!    for every ticket of the pack (the anti-double-claim of the progressive
//!    fill);
//! 5. each wave ends with a notification carrying the cumulative
//!    `claims/total` progress (email + Telegram) — the 20-minute cart window
//!    makes every wave actionable;
//! 6. the request is closed `accepted` once the total is claimed, or `failed`
//!    on a refused shop login; every other error is shop churn, retried on the
//!    next wave.
//!
//! `SeatGone` / `WaitingQueue` are normal resale churn: the seat is skipped,
//! the wave continues with the others, and the request stays `pending` for the
//! next new-seats wave. `Network`, `ExpiredForm` and `MissingContext` are
//! retried later too (the resale page is rendered by BigPipe, so a page without
//! the add-to-cart machinery is usually transient): the wave stops where it
//! stands, whatever was already added is reported, and the request stays
//! `pending`. Only a **refused login** ends the request as `failed`.

use async_trait::async_trait;
use parser::core::seat::Seat;
use parser::interface::curl::checkout::{CheckoutError, ShopSession, SHOP_BASE};

use crate::services::accounts::shop;
use crate::services::bot::purchases::{self, PlanCheckout};
use crate::services::channels::alerts::CartAlert;
use crate::services::notifications;
use crate::services::state::AppState;
use crate::services::tickets;

/// The billetterie cart page where the added seats wait to be paid (the CTA
/// of every cart notification).
pub const CART_URL: &str = "https://billetterie.staderochelais.com";

/// A shop login succeeded: the callable session used to add packs.
#[async_trait]
pub trait CartSession: Send {
    /// Adds one whole resale pack to the session's cart (the caller guarantees
    /// the pack is still listed by [étape 1](crate::services::bot)).
    ///
    /// # Arguments
    /// * `resale_url` — the resale product page
    /// * `pack_id` — the atomic pack to add
    /// * `tickets` — every ticket id of that pack
    /// * `price_range` — `price_slider` fallback when the page does not expose
    ///   its lazily built slider
    ///
    /// # Errors
    /// [`CheckoutError`] — `SeatGone`/`WaitingQueue` are retry-later churn,
    /// the others end the attempt.
    async fn add_pack(
        &mut self,
        resale_url: &str,
        pack_id: u64,
        tickets: &[String],
        price_range: (String, String),
    ) -> Result<(), CheckoutError>;
}

/// Factory of shop sessions, injected into [`AppState`] so the orchestrator is
/// testable without the network (the real implementation wraps the blocking
/// [`ShopSession`] in `spawn_blocking`).
#[async_trait]
pub trait CartShop: Send + Sync {
    /// Authenticates on the billetterie.
    ///
    /// # Errors
    /// [`CheckoutError::Login`] when the credentials are refused (terminal for
    /// the request), [`CheckoutError::WaitingQueue`] / network errors when the
    /// shop is momentarily unavailable (retried on a later wave).
    async fn login(
        &self,
        email: &str,
        password: &str,
    ) -> Result<Box<dyn CartSession>, CheckoutError>;
}

/// Production [`CartShop`]: drives the real billetterie through the curl-based
/// [`ShopSession`] of the parser crate (blocking → `spawn_blocking`).
pub struct RealCartShop;

#[async_trait]
impl CartShop for RealCartShop {
    async fn login(
        &self,
        email: &str,
        password: &str,
    ) -> Result<Box<dyn CartSession>, CheckoutError> {
        let email = email.to_string();
        let password = password.to_string();
        let session = tokio::task::spawn_blocking(move || {
            ShopSession::login(SHOP_BASE, &email, &password).map(jar_session)
        })
        .await
        .map_err(|e| CheckoutError::Network(format!("login task failed: {e}")))??;
        Ok(session)
    }
}

/// Wraps a live [`ShopSession`] into the async [`CartSession`] contract; the
/// curl call runs on the blocking pool.
fn jar_session(session: ShopSession) -> Box<dyn CartSession> {
    Box::new(RealSession(std::sync::Arc::new(session)))
}

/// [`CartSession`] over the real blocking shop session.
struct RealSession(std::sync::Arc<ShopSession>);

#[async_trait]
impl CartSession for RealSession {
    async fn add_pack(
        &mut self,
        resale_url: &str,
        pack_id: u64,
        tickets: &[String],
        price_range: (String, String),
    ) -> Result<(), CheckoutError> {
        let session = std::sync::Arc::clone(&self.0);
        let url = resale_url.to_string();
        let tickets = tickets.to_vec();
        tokio::task::spawn_blocking(move || session.add_pack(&url, pack_id, &tickets, price_range))
            .await
            .map_err(|e| CheckoutError::Network(format!("add-to-cart task failed: {e}")))?
    }
}

/// What the wave was acting on (the polled encounter), for logs and the
/// notification text.
#[derive(Debug, Clone)]
pub struct CheckoutTarget {
    /// LNR match id (already resolved by the scan).
    pub match_id: i64,
    /// Encounter title from the billetterie (opponent display name).
    pub title: String,
    /// Encounter date, free-form French string from the site.
    pub date: String,
    /// The resale product page the seats come from.
    pub resale_link: String,
}

// ============================== EXECUTION ==============================

/// Whether a checkout error is environmental churn worth retrying on a later
/// wave, rather than a reason to close the user's request.
///
/// `SeatGone` (pack taken by somebody else), `WaitingQueue` (shop under load),
/// `Network` (transport hiccup), `ExpiredForm` (stale Drupal form or session)
/// and `MissingContext` (the BigPipe page arrived without its add-to-cart
/// machinery) all leave the request `pending`: the next new-seats wave logs in
/// again and re-adds whatever is still listed. A refused login is the only
/// terminal error left.
fn is_retryable(error: &CheckoutError) -> bool {
    matches!(
        error,
        CheckoutError::SeatGone
            | CheckoutError::WaitingQueue
            | CheckoutError::Network(_)
            | CheckoutError::ExpiredForm
            | CheckoutError::MissingContext
    )
}

/// Executes a wave of plans, strictly one after the other.
///
/// # Arguments
/// * `state` — pool, shop key and the injected [`CartShop`].
/// * `target` — the encounter the seats come from (notification text).
/// * `plans` — matcher decisions for this wave, in serving order.
pub async fn execute_plans(state: &AppState, target: &CheckoutTarget, plans: Vec<PlanCheckout>) {
    for plan in plans {
        execute_plan(state, target, &plan).await;
    }
}

/// Executes one plan: credentials, login, seat adds, claims, notification and
/// final status. Never panics and never propagates errors — every failure is
/// logged and reflected in the request status.
async fn execute_plan(state: &AppState, target: &CheckoutTarget, plan: &PlanCheckout) {
    let user_id = plan.user_id;
    let match_id = plan.match_id;
    if plan.seats.is_empty() {
        return;
    }

    if state.cart_dry_run {
        // Presence only: the vault is read so a dry run tells whether a real
        // run could even log in, but nothing is sent to the shop.
        let credentials =
            match shop::load_credentials(&state.pool, user_id, state.shop_key.as_ref()).await {
                Ok(Some(_)) => "present",
                Ok(None) => "missing",
                Err(_) => "unreadable",
            };
        tracing::info!(
            user_id,
            match_id,
            kind = plan.kind.as_str(),
            seats = plan.seats.len(),
            total = plan.total,
            credentials,
            "checkout dry-run: would add seats to the user's cart (CART_DRY_RUN)"
        );
        return;
    }

    let credentials =
        match shop::load_credentials(&state.pool, user_id, state.shop_key.as_ref()).await {
            Ok(Some(credentials)) => credentials,
            Ok(None) => {
                tracing::debug!(
                    user_id,
                    match_id,
                    "checkout skipped: no shop credentials stored"
                );
                return;
            }
            Err(e) => {
                // Fail-closed (rotated/missing SECRET_KEY): nothing can be decrypted,
                // the request stays pending and the user may re-save credentials.
                tracing::warn!(
                    user_id,
                    match_id,
                    "checkout skipped: shop credentials unreadable ({e})"
                );
                return;
            }
        };

    let mut session = match state
        .cart_shop
        .login(&credentials.email, &credentials.password)
        .await
    {
        Ok(session) => session,
        Err(e) if is_retryable(&e) => {
            // Shop unavailable (queue, transport): the seats are not the
            // problem, the next wave tries again. Nothing is sent.
            tracing::debug!(user_id, match_id, "checkout login postponed: {e}");
            return;
        }
        Err(e) => {
            tracing::warn!(user_id, match_id, "checkout login failed: {e}");
            // Login refused: nothing can be added, this is terminal. The alert
            // still reports what earlier waves left in the cart.
            close_request(state, user_id, match_id, false).await;
            let claimed = claimed_total(state, match_id, user_id).await.unwrap_or(0);
            let alert = wave_alert(target, 0, claimed, plan.total, true);
            notifications::notify_cart_ready(state, user_id, &alert).await;
            return;
        }
    };

    let mut added = 0usize;
    let mut hard_failure = false;
    for (pack_id, tickets) in group_packs(&plan.seats) {
        let price_range = pack_price_range(&plan.seats, pack_id);
        match session
            .add_pack(&target.resale_link, pack_id, &tickets, price_range)
            .await
        {
            Ok(()) => {
                added += tickets.len();
                for ticket in &tickets {
                    match purchases::claim_seat(&state.pool, match_id, pack_id, ticket, user_id)
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => tracing::warn!(
                            user_id,
                            match_id,
                            pack_id,
                            ticket_id = %ticket,
                            "checkout: seat added but already claimed in DB (site won the race)"
                        ),
                        Err(e) => tracing::warn!("checkout: failed to record claim: {e}"),
                    }
                }
                tracing::info!(
                    user_id,
                    match_id,
                    pack_id,
                    seats = tickets.len(),
                    "checkout: pack added to cart"
                );
            }
            Err(CheckoutError::SeatGone) | Err(CheckoutError::WaitingQueue) => {
                tracing::debug!(
                    user_id,
                    match_id,
                    pack_id,
                    seats = tickets.len(),
                    "checkout: pack skipped (resale churn)"
                );
            }
            Err(e) if is_retryable(&e) => {
                // Network / expired form: the shop is momentarily unusable.
                // Stop here; what was already added stays reported, and the
                // request keeps waiting for the next wave.
                tracing::debug!(user_id, match_id, "checkout wave postponed: {e}");
                break;
            }
            Err(e) => {
                // Safety net: every add error is retryable today (a refused
                // login never reaches this loop), so this branch only guards
                // against a future terminal variant.
                tracing::warn!(user_id, match_id, "checkout add failed: {e}");
                hard_failure = true;
                break;
            }
        }
    }

    // Nothing made it into the cart: no message to send, the request keeps
    // waiting for the next wave.
    if added == 0 && !hard_failure {
        tracing::debug!(user_id, match_id, "checkout wave added nothing");
        return;
    }

    // The notification reports the cart content, not the wave delta: the same
    // cumulative count drives the completion check below, so the progress the
    // user reads is the one that closes the request.
    let claims = claimed_total(state, match_id, user_id).await;

    // Per-wave notification: the 20-minute cart window makes it actionable.
    // An unreadable count falls back to the wave delta for the alert only.
    let alert = wave_alert(
        target,
        added,
        claims.unwrap_or(added),
        plan.total,
        hard_failure,
    );
    notifications::notify_cart_ready(state, user_id, &alert).await;

    if hard_failure {
        close_request(state, user_id, match_id, false).await;
        return;
    }

    // Completion: the ordered total is in the cart. Only a count that was
    // actually read can close the request.
    match claims {
        Some(claimed) if claimed >= usize::try_from(plan.total).unwrap_or(usize::MAX) => {
            close_request(state, user_id, match_id, true).await;
        }
        Some(claimed) => tracing::debug!(
            user_id,
            match_id,
            claimed,
            total = plan.total,
            "checkout wave done, request still pending"
        ),
        None => tracing::debug!(
            user_id,
            match_id,
            "checkout wave done, completion postponed (claim count unreadable)"
        ),
    }
}

/// Closes a request (`accepted` on completion, `failed` on a terminal error);
/// a request already moved by somebody else only logs.
async fn close_request(state: &AppState, user_id: i64, match_id: i64, accepted: bool) {
    let moved = if accepted {
        tickets::accept(&state.pool, user_id, match_id).await
    } else {
        tickets::fail(&state.pool, user_id, match_id).await
    };
    match moved {
        Ok(true) => tracing::info!(
            user_id,
            match_id,
            status = if accepted { "accepted" } else { "failed" },
            "checkout: request closed"
        ),
        Ok(false) => tracing::debug!(
            user_id,
            match_id,
            "checkout: request already left pending, status not rewritten"
        ),
        Err(e) => tracing::warn!("checkout: failed to close request: {e}"),
    }
}

/// Groups a plan's seats into their atomic resale packs, preserving the
/// listing order. The billetterie cannot split a pack (a 2-ticket resale is
/// added or lost as a whole), so each returned entry is one add-to-cart POST.
///
/// # Arguments
/// * `seats` — the plan's seats, one entry per ticket
/// # Returns
/// `(pack_id, ticket ids)` pairs, tickets sorted for a deterministic payload
fn group_packs(seats: &[Seat]) -> Vec<(u64, Vec<String>)> {
    let mut packs: Vec<(u64, Vec<String>)> = Vec::new();
    for seat in seats {
        match packs
            .iter_mut()
            .find(|(pack_id, _)| *pack_id == seat.id.pack_id)
        {
            Some((_, tickets)) => tickets.push(seat.id.ticket_id.clone()),
            None => packs.push((seat.id.pack_id, vec![seat.id.ticket_id.clone()])),
        }
    }
    for (_, tickets) in &mut packs {
        tickets.sort_unstable();
    }
    packs
}

/// The pack's price span as printed on the page (`49.00`, `59.00`), used as the
/// `price_slider` fallback when the resale page does not expose the lazily
/// built slider.
///
/// # Arguments
/// * `seats` — the plan's seats
/// * `pack_id` — the pack whose seats are considered
/// # Returns
/// `(min, max)` with two decimals, or two empty strings when no price parses
fn pack_price_range(seats: &[Seat], pack_id: u64) -> (String, String) {
    let prices: Vec<f64> = seats
        .iter()
        .filter(|seat| seat.id.pack_id == pack_id)
        .filter_map(|seat| seat.price.replace(',', ".").parse::<f64>().ok())
        .collect();
    match (
        prices.iter().copied().reduce(f64::min),
        prices.iter().copied().reduce(f64::max),
    ) {
        (Some(min), Some(max)) => (format!("{min:.2}"), format!("{max:.2}")),
        _ => (String::new(), String::new()),
    }
}

/// Builds the wave notification payload.
///
/// # Arguments
/// * `target` — the polled encounter (titles, links of the notification)
/// * `added` — seats this wave put into the cart
/// * `claimed` — seats in the cart after the wave (cumulative claims)
/// * `total` — seats ordered for the match
/// * `failed` — the wave ended on a terminal checkout failure
fn wave_alert(
    target: &CheckoutTarget,
    added: usize,
    claimed: usize,
    total: u32,
    failed: bool,
) -> CartAlert {
    CartAlert {
        opponent: target.title.clone(),
        date: target.date.clone(),
        match_link: target.resale_link.clone(),
        cart_link: CART_URL.to_string(),
        added,
        claimed,
        total,
        failed,
    }
}

/// Cumulative seats the user holds in the cart for this match — the number the
/// notification and the completion check report as `claimed`.
///
/// Reads the same source as the completion check
/// ([`purchases::claimed_count`]) so the `claimed/total` progress is the one
/// that closes the request.
///
/// # Arguments
/// * `state` — pool holding the claims
/// * `match_id` — match whose seats are counted
/// * `user_id` — cart owner
///
/// # Returns
/// `Some(count)` on success, `None` when the query failed (warned here) — the
/// caller must then not close the request on that number.
async fn claimed_total(state: &AppState, match_id: i64, user_id: i64) -> Option<usize> {
    match purchases::claimed_count(&state.pool, match_id, user_id).await {
        Ok(count) => usize::try_from(count).ok(),
        Err(e) => {
            tracing::warn!(
                user_id,
                match_id,
                "checkout: failed to read claim count: {e}"
            );
            None
        }
    }
}
