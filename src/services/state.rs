//! Shared dependency container handed to every Axum handler and background
//! service — the single place wiring implementations to traits.

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::common::crypto::SecretKey;
use crate::services::bot::checkout::CartShop;
use crate::services::calendar::CalendarService;
use crate::services::channels::mailer::Mailer;
use crate::services::channels::telegram::{Telegram, TelegramConfig};
use crate::services::rate_limit::RateLimiter;

/// All dependencies of the app, cheap-to-clone (pools and `Arc`s only).
///
/// Concrete transports are injected behind traits ([`Mailer`], [`Telegram`],
/// [`CartShop`]) so tests swap in fakes; the config values live alongside the
/// clients because handlers need the bot username/deep link without an HTTP
/// client.
#[derive(Clone)]
pub struct AppState {
    /// Shared SQLite connection pool (also used by background services).
    pub pool: SqlitePool,
    /// Welcome-email and match-alert transport.
    pub mailer: Arc<dyn Mailer>,
    /// Telegram transport (a no-op when `TELEGRAM_ENABLED` is not `true`).
    pub telegram: Arc<dyn Telegram>,
    /// Static Telegram settings: bot username for the linking deep link.
    pub telegram_config: TelegramConfig,
    /// Shared in-memory calendar cache used by reads and the bot scanner.
    pub calendar: Arc<CalendarService>,
    /// AES key for the shop-credentials vault, derived from `SECRET_KEY` at
    /// startup; `None` refuses the credentials routes (fail-closed) and later
    /// the whole checkout feature.
    pub shop_key: Option<SecretKey>,
    /// `CART_ENABLED` — runs the checkout orchestrator at all.
    pub cart_enabled: bool,
    /// `CART_DRY_RUN` — log the planned checkouts without touching the shop
    /// (no login, no add-to-cart, no claim, no notification).
    pub cart_dry_run: bool,
    /// Shop session factory the orchestrator drives (`RealCartShop` in
    /// production, fakes in tests).
    pub cart_shop: Arc<dyn CartShop>,
    /// Brute-force guard for the public auth endpoints (`register` / `login`)
    /// — in-memory sliding windows keyed by client IP.
    pub rate_limiter: Arc<RateLimiter>,
    /// `PROXY_TRUSTED` — when `true`, the rate limiter identifies clients by
    /// the `CF-Connecting-IP` header (Cloudflare tunnel, overwritten by the
    /// edge) or the first `X-Forwarded-For` entry (classic reverse proxy);
    /// otherwise the socket peer IP is used.
    pub proxy_trusted: bool,
}
