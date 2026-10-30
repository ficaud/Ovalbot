//! Shop credential routes: the per-user billetterie vault behind
//! `/api/shop/credentials` (the Phase 4B automatic add-to-cart prerequisite).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::common::error::AppError;
use crate::services::accounts::auth::ActiveUser;
use crate::services::accounts::shop;
use crate::services::state::AppState;

/// `PUT /api/shop/credentials` payload. Both fields are required; the shop
/// email is intentionally independent from the OvalBot account email (the
/// billetterie account may differ).
#[derive(Debug, Deserialize)]
pub struct ShopCredentialsRequest {
    pub email: String,
    pub password: String,
}

/// `PUT /api/shop/credentials` — validates then stores (or replaces) the
/// current user's billetterie credentials. The clear password is encrypted
/// server-side (`AES-256-GCM`, `SECRET_KEY`-derived key) and never echoed
/// back, logged or serialized.
///
/// # Returns
/// `204` on success.
///
/// # Errors
/// `AppError::ShopCredentialsUnavailable` (503) when `SECRET_KEY` is not
/// configured — fail-closed: no clear/encrypted substitute is ever stored,
/// `AppError::InvalidShopCredentials` (400) on empty/invalid input,
/// `AppError::Sqlx`/`Crypto` internally otherwise.
pub async fn set_credentials(
    State(state): State<AppState>,
    auth: ActiveUser,
    Json(body): Json<ShopCredentialsRequest>,
) -> Result<Response, AppError> {
    if body.email.trim().is_empty()
        || !body.email.contains('@')
        || body.password.is_empty()
        || body.password.len() > 1024
    {
        return Err(AppError::InvalidShopCredentials);
    }

    shop::set_credentials(
        &state.pool,
        auth.0.user.id,
        &body.email,
        &body.password,
        state.shop_key.as_ref(),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `DELETE /api/shop/credentials` — clears the current user's stored
/// billetterie credentials (idempotent).
///
/// # Returns
/// `204` on success.
///
/// # Errors
/// `AppError::Sqlx` on DB errors.
pub async fn clear_credentials(
    State(state): State<AppState>,
    auth: ActiveUser,
) -> Result<Response, AppError> {
    shop::clear_credentials(&state.pool, auth.0.user.id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
