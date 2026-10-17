use axum::extract::Path;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::common::error::AppError;
use crate::services::auth::ActiveUser;

// Defensive upper bound for a ticket price, in cents (1000 €).
const MAX_PRICE_CENTS: u32 = 100_000;

// Ticket request parameters sent by the frontend for a single match.
#[derive(Debug, Deserialize)]
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
}

fn is_valid_price(cents: u32) -> bool {
    (1..=MAX_PRICE_CENTS).contains(&cents)
}

/// Store the ticket request parameters for a match.
/// No persistence yet: the payload is validated and logged, the bot integration comes later.
pub async fn put_request(
    auth: ActiveUser,
    Path(match_id): Path<i64>,
    Json(input): Json<TicketRequestInput>,
) -> Result<StatusCode, AppError> {
    if !input.is_valid() {
        return Err(AppError::InvalidTicketRequest);
    }
    tracing::info!(
        user_id = auth.0.user.id,
        match_id,
        request = ?input,
        "ticket request received"
    );
    Ok(StatusCode::NO_CONTENT)
}

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
}
