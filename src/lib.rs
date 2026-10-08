pub mod auth;
pub mod db;
pub mod email;
pub mod error;
pub mod mailer;
pub mod models;
pub mod routes;
pub mod state;
pub mod time;
pub mod waitlist;

use std::path::PathBuf;

use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::routes::{admin, auth as auth_routes};
use crate::state::AppState;

pub fn app(state: AppState) -> Router {
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "frontend/build".to_owned());
    let index = PathBuf::from(&static_dir).join("index.html");

    Router::new()
        .route("/api/register", post(auth_routes::register))
        .route("/api/login", post(auth_routes::login))
        .route("/api/logout", post(auth_routes::logout))
        .route("/api/me", get(auth_routes::me))
        .route("/api/version", get(auth_routes::version))
        .route("/api/admin/users", get(admin::list_users))
        .route("/api/admin/users/{id}/activate", post(admin::activate_user))
        .route("/api/admin/users/{id}", delete(admin::delete_user))
        .fallback_service(ServeDir::new(&static_dir).fallback(ServeFile::new(index)))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
