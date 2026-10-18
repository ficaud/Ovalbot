use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::common::error::AppError;

// ============================== TICKET INPUT FROM FRONTEND ==============================

// Defensive upper bound for a ticket price, in cents (1000 €).
const MAX_PRICE_CENTS: u32 = 100_000;

// Ticket request parameters sent by the frontend for a single match in Json.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TicketRequestInput {
    Single { quantity: u8, max_price_cents: u32 },
    Duo { max_price_cents: u32 },
    Group { places: u8 },
}

impl TicketRequestInput {
    // Bounds mirrored from the frontend modal.
    fn is_valid(&self) -> bool {
        match self {
            Self::Single {
                quantity,
                max_price_cents,
            } => (1..=10).contains(quantity) && is_valid_price(*max_price_cents),
            Self::Duo { max_price_cents } => is_valid_price(*max_price_cents),
            Self::Group { places } => (2..=10).contains(places),
        }
    }

    // Flattened representation of the request, following the `ticket_requests` columns.
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
        }
    }
}

/// Checks if the price is valid (not too high).
fn is_valid_price(cents: u32) -> bool {
    (1..=MAX_PRICE_CENTS).contains(&cents)
}

// ============================== TICKET REQUEST TYPES ==============================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum TicketKind {
    Single,
    Duo,
    Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum TicketStatus {
    Pending,
    Accepted,
    Purchased,
    Failed,
    Expired,
    Cancelled,
}

/// A stored ticket request as returned by the API.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct TicketRequestView {
    pub match_id: i64,
    pub kind: TicketKind,
    pub quantity: Option<u8>,
    pub max_price_cents: Option<u32>,
    pub places: Option<u8>,
    pub status: TicketStatus,
    pub created_at: String,
    pub updated_at: String,
}

/// Outcome of a removal: the row may be gone, absent, or protected by its status.
pub enum RemoveOutcome {
    Removed,
    NotFound,
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
    }
}
