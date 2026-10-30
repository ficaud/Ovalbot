//! OvalBot backend: an Axum HTTP server (the site + API) with an embedded
//! SvelteKit frontend, SQLite persistence, and background services
//! (calendar sync, match notifications, Telegram, ticket resale bot).
//!
//! The crate owns no business logic itself — it wires [`crate::common`]
//! foundations, [`crate::services`] (business + integrations) and
//! [`crate::routes`] (HTTP handlers) together via [`app`].

pub mod common;
pub mod routes;
pub mod services;

use std::path::PathBuf;

use axum::routing::{delete, get, post, put};
use axum::Router;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::routes::{
    admin, auth as auth_routes, calendar as calendar_routes, notifications as notifications_routes,
    shop as shop_routes, telegram as telegram_routes, tickets as tickets_routes,
};
use crate::services::state::AppState;

/// Builds the full HTTP application: every API route plus static serving of
/// the pre-built SvelteKit frontend (`STATIC_DIR`, default `frontend/build`)
/// with `index.html` as the SPA fallback.
///
/// The route table mirrors the frontend's `/api` proxy — auth/account,
/// calendar reads, match-notification subscriptions and settings, ticket
/// requests, Telegram linking/webhook, admin user management.
///
/// # Arguments
/// * `state` — shared dependencies (pool, mailer, telegram, calendar) handed
///   to all handlers.
///
/// # Returns
/// An `axum::Router` ready to be served as-is.
pub fn app(state: AppState) -> Router {
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "frontend/build".to_owned());
    let index = PathBuf::from(&static_dir).join("index.html");

    Router::new()
        .route("/api/register", post(auth_routes::register))
        .route("/api/login", post(auth_routes::login))
        .route("/api/logout", post(auth_routes::logout))
        .route("/api/me", get(auth_routes::me))
        .route("/api/version", get(auth_routes::version))
        .route("/api/teams", get(calendar_routes::list_teams))
        .route(
            "/api/teams/{team}/matches",
            get(calendar_routes::team_matches),
        )
        .route("/api/notifications", get(notifications_routes::list))
        .route(
            "/api/notifications/settings",
            put(notifications_routes::update_settings),
        )
        .route(
            "/api/notifications/{match_id}",
            post(notifications_routes::subscribe).delete(notifications_routes::unsubscribe),
        )
        .route("/api/requests", get(tickets_routes::list_requests))
        .route(
            "/api/requests/{match_id}",
            put(tickets_routes::new_ticket_request).delete(tickets_routes::delete_ticket_request),
        )
        .route(
            "/api/telegram/link",
            post(telegram_routes::link).delete(telegram_routes::unlink),
        )
        .route(
            "/api/shop/credentials",
            put(shop_routes::set_credentials).delete(shop_routes::clear_credentials),
        )
        .route("/api/telegram/webhook", post(telegram_routes::webhook))
        .route("/api/admin/users", get(admin::list_users))
        .route("/api/admin/users/{id}/activate", post(admin::activate_user))
        .route("/api/admin/users/{id}", delete(admin::delete_user))
        .fallback_service(ServeDir::new(&static_dir).fallback(ServeFile::new(index)))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
