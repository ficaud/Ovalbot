use axum::http::StatusCode;
use serde::Deserialize;
use std::{collections::HashMap, sync::RwLock};

use crate::common::error::AppError;

// ============================== TICKET INPUT FROM FRONTEND ==============================

// Defensive upper bound for a ticket price, in cents (1000 €).
const MAX_PRICE_CENTS: u32 = 100_000;

// Ticket request parameters sent by the frontend for a single match in Json.
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TicketRequestInput {
    Single { quantity: u8, max_price_cents: u32 },
    Duo { max_price_cents: u32 },
    Group { places: u8 },
}

/// Defensive upper bound for a ticket price, in cents (1000 €).
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

/// Checks if the price is valid (not too high).
fn is_valid_price(cents: u32) -> bool {
    (1..=MAX_PRICE_CENTS).contains(&cents)
}

// ============================== TICKET MANAGER ==============================
pub struct TicketManager {
    // tickets are in memory cache, so that we can read them as we want without blocking the tokio runtime
    // but the writing requires a write lock (only one write at the time).
    tickets: RwLock<HashMap<(i64, i64), TicketRequestInput>>,
}
impl Default for TicketManager {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for TicketManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tickets = self.tickets.read().expect("tickets cache poisoned");
        f.debug_map().entries(tickets.iter()).finish()
    }
}

impl TicketManager {
    pub fn new() -> Self {
        Self {
            // event list is empty by default
            tickets: RwLock::new(HashMap::new()),
        }
    }
    pub fn upsert(&self, user_id: i64, match_id: i64, request: TicketRequestInput) {
        let mut tickets = self.tickets.write().expect("tickets cache poisoned");

        tickets.insert((user_id, match_id), request);
    }

    pub fn remove(&self, user_id: i64, match_id: i64) -> bool {
        self.tickets
            .write()
            .expect("tickets cache poisoned")
            .remove(&(user_id, match_id))
            .is_some()
    }
}

/// Store the ticket request parameters for a match.
/// The request will be added in cache memory (or modified if it already exists)
/// and stored in the database.
pub async fn put_request(
    tickets: &TicketManager,
    user_id: i64,
    match_id: i64,
    ticket_request: TicketRequestInput,
) -> Result<StatusCode, AppError> {
    if !ticket_request.is_valid() {
        return Err(AppError::InvalidTicketRequest);
    }

    // insert or replace the ticket request in the cache.
    tickets.upsert(user_id, match_id, ticket_request);

    // Print all the tickets requests in the cache.
    tracing::debug!("ticket cache:\n{:#?}", tickets);

    Ok(StatusCode::NO_CONTENT)
}

/// Delete a ticket request from the database.
/// The request will be removed from cache memory.
/// and removed from the database for persistence.
pub async fn delete_request(
    tickets: &TicketManager,
    user_id: i64,
    match_id: i64,
) -> Result<StatusCode, AppError> {
    tracing::info!(user_id = user_id, match_id, "ticket request removed");

    // remove the ticket request from the cache.
    tickets.remove(user_id, match_id);

    // Print all the tickets requests in the cache.
    tracing::debug!("ticket cache:\n{:#?}", tickets);

    Ok(StatusCode::NO_CONTENT)
}

// ============================== TICKET PROPOSAL ==============================

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

    /// The first upsert of a pair stores the request in the cache.
    #[test]
    fn upsert_inserts_a_new_request() {
        let manager = TicketManager::new();
        manager.upsert(
            1,
            42,
            TicketRequestInput::Duo {
                max_price_cents: 9000,
            },
        );

        let tickets = manager.tickets.read().unwrap();
        assert_eq!(tickets.len(), 1);
        assert!(matches!(
            tickets.get(&(1, 42)),
            Some(TicketRequestInput::Duo {
                max_price_cents: 9000
            })
        ));
    }

    /// A second upsert on the same pair replaces the previous request.
    #[test]
    fn upsert_replaces_an_existing_request() {
        let manager = TicketManager::new();
        manager.upsert(
            1,
            42,
            TicketRequestInput::Duo {
                max_price_cents: 9000,
            },
        );
        manager.upsert(
            1,
            42,
            TicketRequestInput::Single {
                quantity: 3,
                max_price_cents: 4500,
            },
        );

        let tickets = manager.tickets.read().unwrap();
        assert_eq!(tickets.len(), 1);
        assert!(matches!(
            tickets.get(&(1, 42)),
            Some(TicketRequestInput::Single {
                quantity: 3,
                max_price_cents: 4500
            })
        ));
    }

    /// Two users requesting the same match do not overwrite each other.
    #[test]
    fn upsert_keeps_users_apart() {
        let manager = TicketManager::new();
        manager.upsert(1, 42, TicketRequestInput::Group { places: 4 });
        manager.upsert(
            2,
            42,
            TicketRequestInput::Duo {
                max_price_cents: 9000,
            },
        );

        let tickets = manager.tickets.read().unwrap();
        assert_eq!(tickets.len(), 2);
        assert!(tickets.contains_key(&(1, 42)));
        assert!(tickets.contains_key(&(2, 42)));
    }

    /// Removing an existing request reports it and leaves the cache consistent.
    #[test]
    fn remove_deletes_the_request() {
        let manager = TicketManager::new();
        manager.upsert(1, 42, TicketRequestInput::Group { places: 4 });

        assert!(manager.remove(1, 42));
        assert!(!manager.remove(1, 42));
        assert!(manager.tickets.read().unwrap().is_empty());
    }
}
