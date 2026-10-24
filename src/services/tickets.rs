//! Ticket requests: the per-user, per-match "what I want to buy" parameters
//! persisted in SQLite (`ticket_requests`), validated server-side against the
//! bounds the frontend modal enforces.
//!
//! The rows double as the bot's matching source: an accepted request can be
//! switched out of `pending` (accepted/purchased/…) by the Phase 4 flow, and
//! the atomic conditional `UPDATE`s here guarantee a claim can't race.
//! APIs: `upsert` / `remove` / `list_for_user` (called by `routes::tickets`),
//! `pending_requesters` + `record_offers` (called by the bot scan loop: who to
//! alert, and which resale seats are genuinely new — the durable anti-spam
//! guard over `ticket_offers`).
//!
//! One stored kind is not a buying intent: `kind = "notify"` subscribes the
//! user to the resale-seats alerts of a single match with no bot involvement
//! (the Phase 4 matcher must not serve those rows). Alert candidates are
//! **only** these subscriptions — the buying kinds never receive alerts
//! themselves; engaging the buy bot (Phase 4) is the only path a buying
//! request takes.

use parser::core::seat::Seat;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::common::error::AppError;

// ============================== TICKET INPUT FROM FRONTEND ==============================

// Defensive upper bound for a ticket price, in cents (1000 €).
const MAX_PRICE_CENTS: u32 = 100_000;

/// Per-match ticket request, tagged on the buying mode (`{"kind": ...}`).
///
/// Each kind fills exactly the columns it needs (`columns()`), the others are
/// NULL — kills the ambiguity of one generic row shape.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TicketRequestInput {
    /// One-at-a-time buying (`quantity` in 1..=10 tickets, each ≤
    /// `max_price_cents`), bought as they become available.
    Single { quantity: u8, max_price_cents: u32 },
    /// A single pair, priced as a whole (`max_price_cents` for the duo).
    Duo { max_price_cents: u32 },
    /// 2..=10-standing seats on a shared billing; the pelouse rate applies
    /// automatically, so no price is requested.
    Group { places: u8 },
    /// Notification-only subscription: the user wants to be alerted about
    /// resale seats for the match but must not be served by the buy bot
    /// (Phase 4 matcher has to skip this kind). No payload columns.
    Notify,
}

impl TicketRequestInput {
    // Bounds mirrored from the frontend modal.
    /// Validates the variant against the same bounds the frontend enforces
    /// (Single: quantity 1..=10 and price 1..=[`MAX_PRICE_CENTS`]; Duo:
    /// price; Group: places 2..=10; Notify: payloadless, always valid).
    ///
    /// # Returns
    /// `true` when the payload may be stored; the route answers 400 otherwise.
    fn is_valid(&self) -> bool {
        match self {
            Self::Single {
                quantity,
                max_price_cents,
            } => (1..=10).contains(quantity) && is_valid_price(*max_price_cents),
            Self::Duo { max_price_cents } => is_valid_price(*max_price_cents),
            Self::Group { places } => (2..=10).contains(places),
            Self::Notify => true,
        }
    }

    /// Flattens the variant into the `ticket_requests` columns
    /// `(kind, quantity, max_price_cents, places)` — exactly one of the three
    /// payload columns is `Some`, per the SQL CHECK constraints (`Notify`
    /// leaves all three `None`).
    ///
    /// # Returns
    /// The column tuple to bind in the upsert statement.
    fn columns(&self) -> (TicketKind, Option<u8>, Option<u32>, Option<u8>) {
        match self {
            Self::Single {
                quantity,
                max_price_cents,
            } => (
                TicketKind::Single,
                Some(*quantity),
                Some(*max_price_cents),
                None,
            ),
            Self::Duo { max_price_cents } => (TicketKind::Duo, None, Some(*max_price_cents), None),
            Self::Group { places } => (TicketKind::Group, None, None, Some(*places)),
            Self::Notify => (TicketKind::Notify, None, None, None),
        }
    }
}

/// Checks if the price is valid (not too high).
fn is_valid_price(cents: u32) -> bool {
    (1..=MAX_PRICE_CENTS).contains(&cents)
}

// ============================== TICKET REQUEST TYPES ==============================

/// Buying mode of a stored request — serialized as `"single"`/`"duo"`/`"group"`/`"notify"`
/// and stored with the same spelling (sqlx `rename_all`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum TicketKind {
    Single,
    Duo,
    Group,
    /// Notification-only subscription: no payload columns, and the buy bot
    /// (Phase 4 matcher) must skip it — the user only asked to be alerted.
    Notify,
}

/// Lifecycle of a request. Fresh rows are `Pending`; the Phase 4 claim flow
/// moves them out of it (upsert/remove refuse everything but `Pending`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum TicketStatus {
    /// Waiting for a matching offer; still modifiable.
    Pending,
    /// An offer matched and was claimed.
    Accepted,
    /// Tickets bought; terminal success.
    Purchased,
    /// The attempt failed terminally.
    Failed,
    /// Expired without ever being claimed.
    Expired,
    /// Withdrawn by the user (or removed after a bot claim attempt).
    Cancelled,
}

/// One stored request as serialized by `GET /api/requests` (flat shape; the
/// payload columns unused by `kind` are `None`).
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct TicketRequestView {
    /// LNR match id the request targets (the bot resolves offers to it too).
    pub match_id: i64,
    /// Buying mode; drives which payload columns are set.
    pub kind: TicketKind,
    /// `Single` only: how many individual tickets are wanted (1..=10).
    pub quantity: Option<u8>,
    /// `Single`/`Duo` only: per-ticket / per-duo budget in cents.
    pub max_price_cents: Option<u32>,
    /// `Group` only: how many pelouse places (2..=10, shared billing).
    pub places: Option<u8>,
    /// Lifecycle state (`pending` until Phase 4 claims or closes it).
    pub status: TicketStatus,
    /// First-insert timestamp (RFC 3339), preserved across updates.
    pub created_at: String,
    /// Last modification timestamp (RFC 3339).
    pub updated_at: String,
}

/// Outcome of [`remove`]: distinguishes the plain "ok" from the two requests
/// that leave the DB untouched (missing row vs no longer modifiable).
pub enum RemoveOutcome {
    /// The `pending` row was deleted.
    Removed,
    /// No row matched; the HTTP layer answers 204 anyway (idempotent).
    NotFound,
    /// The row exists but left `pending`; modification is refused.
    Locked,
}

// ============================== SERVICE ==============================

/// Insert the request, or update the existing one while it is still `pending`.
///
/// `created_at` is preserved on update. Returns `TicketRequestLocked` when the
/// stored request has already left the `pending` state.
pub async fn upsert(
    pool: &SqlitePool,
    user_id: i64,
    match_id: i64,
    input: &TicketRequestInput,
) -> Result<TicketRequestView, AppError> {
    if !input.is_valid() {
        return Err(AppError::InvalidTicketRequest);
    }

    let now = crate::common::time::now();
    let (kind, quantity, max_price_cents, places) = input.columns();

    let view = sqlx::query_as::<_, TicketRequestView>(
        "INSERT INTO ticket_requests \
             (user_id, match_id, kind, quantity, max_price_cents, places, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?) \
         ON CONFLICT(user_id, match_id) DO UPDATE SET \
             kind = excluded.kind, \
             quantity = excluded.quantity, \
             max_price_cents = excluded.max_price_cents, \
             places = excluded.places, \
             updated_at = excluded.updated_at \
         WHERE ticket_requests.status = 'pending' \
         RETURNING match_id, kind, quantity, max_price_cents, places, status, created_at, updated_at",
    )
    .bind(user_id)
    .bind(match_id)
    .bind(kind)
    .bind(quantity)
    .bind(max_price_cents)
    .bind(places)
    .bind(&now)
    .bind(&now)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::TicketRequestLocked)?;

    if view.created_at == now {
        tracing::info!(user_id, match_id, "ticket request created");
    } else {
        tracing::info!(user_id, match_id, "ticket request updated");
    }
    Ok(view)
}

/// Delete the request while it is still `pending`.
///
/// A request that left the `pending` state cannot be removed (`Locked`); a
/// missing request is not an error (`NotFound`), so the operation stays
/// idempotent at the HTTP level.
pub async fn remove(
    pool: &SqlitePool,
    user_id: i64,
    match_id: i64,
) -> Result<RemoveOutcome, AppError> {
    let removed = sqlx::query_scalar::<_, i64>(
        "DELETE FROM ticket_requests \
         WHERE user_id = ? AND match_id = ? AND status = 'pending' \
         RETURNING match_id",
    )
    .bind(user_id)
    .bind(match_id)
    .fetch_optional(pool)
    .await?;

    if removed.is_some() {
        tracing::info!(user_id, match_id, "ticket request removed");
        return Ok(RemoveOutcome::Removed);
    }

    // Either the request does not exist, or it is no longer pending.
    let status: Option<TicketStatus> =
        sqlx::query_scalar("SELECT status FROM ticket_requests WHERE user_id = ? AND match_id = ?")
            .bind(user_id)
            .bind(match_id)
            .fetch_optional(pool)
            .await?;

    match status {
        Some(_) => {
            tracing::info!(user_id, match_id, "ticket request locked, removal refused");
            Ok(RemoveOutcome::Locked)
        }
        None => Ok(RemoveOutcome::NotFound),
    }
}

/// List the ticket requests of a single user, ordered by match id.
pub async fn list_for_user(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<TicketRequestView>, AppError> {
    let views = sqlx::query_as::<_, TicketRequestView>(
        "SELECT match_id, kind, quantity, max_price_cents, places, status, created_at, updated_at \
         FROM ticket_requests \
         WHERE user_id = ? \
         ORDER BY match_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(views)
}

/// One user subscribed to a match's seats alerts, fetched by
/// [`pending_requesters`] for the bot: the subscriber's contact + channel
/// gates, plus the raw per-kind columns a future matcher may need. Under the
/// current notify-only filter those payload columns are always `None`.
#[derive(Debug, Clone, PartialEq, FromRow)]
pub struct PendingRequester {
    /// User who made the request (`users.id`).
    pub user_id: i64,
    /// Email address to alert ([`crate::services::channels::mailer::Mailer::send_seats_alert`]).
    pub email: String,
    /// Linked Telegram chat id; unlinked users get nothing on that channel.
    pub telegram_id: Option<String>,
    /// Per-channel gate: the user must accept email alerts.
    pub notify_email: bool,
    /// Per-channel gate: the user must accept Telegram alerts.
    pub notify_telegram: bool,
    /// Buying mode; drives which parameter columns are set.
    pub kind: TicketKind,
    /// `Single` only: how many individual tickets are wanted (1..=10).
    pub quantity: Option<u8>,
    /// `Single`/`Duo` only: per-ticket / per-duo budget in cents.
    pub max_price_cents: Option<u32>,
    /// `Group` only: how many pelouse places (2..=10).
    pub places: Option<u8>,
}

/// Lists the seats-alert subscribers of a single match, ordered by user id.
///
/// The bot scan loop calls this (through
/// [`crate::services::notifications::notify_new_seats`]) when a match's resale
/// gained seats. Only **explicit alert subscriptions** count: `pending` rows
/// with `kind = 'notify'` — the user selected the notification option in the
/// frontend. Buying kinds (`Single` / `Duo` / `Group`) are never alerted;
/// serving them is Phase 4's job. Contact info and the per-channel gates ride
/// along, one query total.
///
/// # Returns
/// One [`PendingRequester`] per subscriber, keyed and ordered by user id;
/// empty when nobody subscribed to the match's alerts.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn pending_requesters(
    pool: &SqlitePool,
    match_id: i64,
) -> Result<Vec<PendingRequester>, AppError> {
    let requesters = sqlx::query_as::<_, PendingRequester>(
        "SELECT tr.user_id, u.email, u.telegram_id, u.notify_email, u.notify_telegram, \
             tr.kind, tr.quantity, tr.max_price_cents, tr.places \
         FROM ticket_requests tr \
         JOIN users u ON u.id = tr.user_id \
         WHERE tr.match_id = ? AND tr.status = 'pending' AND tr.kind = 'notify' \
         ORDER BY tr.user_id",
    )
    .bind(match_id)
    .fetch_all(pool)
    .await?;
    Ok(requesters)
}

/// Site-native identity of one resale seat, as recorded in `ticket_offers`.
#[derive(Debug, Clone, PartialEq)]
pub struct OfferKey {
    /// Product/pack the ticket belongs to on the billetterie.
    pub pack_id: u64,
    /// Site-native ticket id inside the pack.
    pub ticket_id: String,
}

/// Records a match's resale seats durably in `ticket_offers` — the bot's
/// anti-spam guard.
///
/// Image semantics: `ON CONFLICT DO NOTHING` keyed on
/// `(match_id, pack_id, ticket_id)`. Anything already recorded (or recorded by
/// an earlier scan/restart) is ignored; what this call actually inserted is
/// reported back, so a seat can alert the match's requesters exactly once —
/// even across server restarts.
///
/// # Arguments
/// * `title` / `date` — the encounter's display identity, stored as-is from
///   the first time the seat is seen (display-only columns).
/// * `seats` — the seats currently on resale for the encounter.
///
/// # Returns
/// The [`OfferKey`] of the seats inserted by this call (truly new ones).
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn record_offers(
    pool: &SqlitePool,
    match_id: i64,
    title: &str,
    date: &str,
    seats: &[Seat],
) -> Result<Vec<OfferKey>, AppError> {
    let now = crate::common::time::now();
    let mut fresh = Vec::with_capacity(seats.len());
    for seat in seats {
        let key = sqlx::query_as::<_, (i64, String)>(
            "INSERT INTO ticket_offers (match_id, title, date, pack_id, ticket_id, first_seen_at) \
             VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT(match_id, pack_id, ticket_id) DO NOTHING \
             RETURNING pack_id, ticket_id",
        )
        .bind(match_id)
        .bind(title)
        .bind(date)
        .bind(seat.id.pack_id as i64)
        .bind(&seat.id.ticket_id)
        .bind(&now)
        .fetch_optional(pool)
        .await?
        .map(|(pack_id, ticket_id)| OfferKey {
            pack_id: pack_id as u64,
            ticket_id,
        });
        // Rows that hit the conflict clause are neither inserted nor returned.
        if let Some(key) = key {
            fresh.push(key);
        }
    }
    if !fresh.is_empty() {
        tracing::debug!(
            match_id,
            seats = seats.len(),
            fresh = fresh.len(),
            "ticket offers recorded"
        );
    }
    Ok(fresh)
}

// ============================== TICKET PROPOSAL TYPES ==============================

// All the types that the proposal bot can sent to the backend of ovalbot.

// ============================== TICKET PROPOSAL SERVICE ==============================

// All the services that the proposal bot can send to the backend of ovalbot.

// ============================== TESTS ==============================

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind accepts its nominal payload.
    #[test]
    fn accepts_valid_payloads() {
        assert!(TicketRequestInput::Single {
            quantity: 3,
            max_price_cents: 4500
        }
        .is_valid());
        assert!(TicketRequestInput::Duo {
            max_price_cents: 9000
        }
        .is_valid());
        assert!(TicketRequestInput::Group { places: 4 }.is_valid());
        assert!(TicketRequestInput::Notify.is_valid());
    }

    /// Single tickets require a quantity between 1 and 10 and a positive price.
    #[test]
    fn rejects_invalid_single() {
        assert!(!TicketRequestInput::Single {
            quantity: 0,
            max_price_cents: 4500
        }
        .is_valid());
        assert!(!TicketRequestInput::Single {
            quantity: 11,
            max_price_cents: 4500
        }
        .is_valid());
        assert!(!TicketRequestInput::Single {
            quantity: 2,
            max_price_cents: 0
        }
        .is_valid());
        assert!(!TicketRequestInput::Single {
            quantity: 2,
            max_price_cents: MAX_PRICE_CENTS + 1
        }
        .is_valid());
    }

    /// Duo requires a positive price only.
    #[test]
    fn rejects_invalid_duo() {
        assert!(!TicketRequestInput::Duo { max_price_cents: 0 }.is_valid());
        assert!(!TicketRequestInput::Duo {
            max_price_cents: MAX_PRICE_CENTS + 1
        }
        .is_valid());
    }

    /// Group tickets require between 2 and 10 places.
    #[test]
    fn rejects_invalid_group() {
        assert!(!TicketRequestInput::Group { places: 1 }.is_valid());
        assert!(!TicketRequestInput::Group { places: 11 }.is_valid());
    }

    /// Each kind maps to its own columns, leaving the others empty.
    #[test]
    fn columns_maps_each_kind() {
        let (kind, quantity, max_price_cents, places) = TicketRequestInput::Single {
            quantity: 3,
            max_price_cents: 4500,
        }
        .columns();
        assert_eq!(kind, TicketKind::Single);
        assert_eq!(quantity, Some(3));
        assert_eq!(max_price_cents, Some(4500));
        assert_eq!(places, None);

        let (kind, quantity, max_price_cents, places) = TicketRequestInput::Duo {
            max_price_cents: 9000,
        }
        .columns();
        assert_eq!(kind, TicketKind::Duo);
        assert_eq!(quantity, None);
        assert_eq!(max_price_cents, Some(9000));
        assert_eq!(places, None);

        let (kind, quantity, max_price_cents, places) =
            TicketRequestInput::Group { places: 4 }.columns();
        assert_eq!(kind, TicketKind::Group);
        assert_eq!(quantity, None);
        assert_eq!(max_price_cents, None);
        assert_eq!(places, Some(4));

        assert_eq!(
            TicketRequestInput::Notify.columns(),
            (TicketKind::Notify, None, None, None)
        );
    }
}
