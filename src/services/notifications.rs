//! Per-match notification fan-out: scheduling alerts (calendar-driven) and
//! seats-on-resale alerts (bot-driven).
//!
//! Scheduling: `calendar.sync()` reports `Change::Scheduled` flips → the
//! caller passes them to [`process`] → every subscriber of that match gets an
//! email and/or Telegram message (per-channel preferences respected) → the
//! subscriptions for the match are cleared so everybody is notified once.
//!
//! Seats: when the bot records genuinely new resale seats for one match, it
//! calls [`notify_new_seats`] → every `notify`-kind subscriber of that match
//! (explicit opt-in in the frontend) gets an email and/or Telegram message
//! gated by the same per-channel preferences. Buying kinds are never alerted.
//!
//! When the admin configured a broadcast channel, each of these alerts is also
//! mirrored there — once per event — if the admin is among the recipients. The
//! mirror is sent by a dedicated bot whose token the admin pasted in the
//! settings (encrypted at rest, decrypted per send), not by the app's main bot.
//! That mirror is independent of the admin's own channel preferences: their
//! subscriptions decide what gets published.

use std::time::Instant;

use calendar::Change;
use sqlx::SqlitePool;

use crate::common::crypto::decrypt;
use crate::common::error::AppError;
use crate::services::channels::alerts::{CartAlert, MatchAlert, SeatAlert};
use crate::services::channels::telegram;
use crate::services::state::AppState;
use crate::services::tickets;

/// Subscribes a user to a match: records the row in `match_notifications`
/// (idempotent — an existing subscription is kept via `DO NOTHING`).
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn subscribe(pool: &SqlitePool, user_id: i64, match_id: i64) -> Result<(), AppError> {
    let now = crate::common::time::now();
    sqlx::query(
        "INSERT INTO match_notifications (user_id, match_id, created_at) VALUES (?, ?, ?) \
         ON CONFLICT(user_id, match_id) DO NOTHING",
    )
    .bind(user_id)
    .bind(match_id)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Removes a user's subscription to a match (idempotent: deleting an absent
/// row succeeds).
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn unsubscribe(pool: &SqlitePool, user_id: i64, match_id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM match_notifications WHERE user_id = ? AND match_id = ?")
        .bind(user_id)
        .bind(match_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Lists every match id the user subscribed to, ascending.
///
/// # Returns
/// One LNR match id per subscription row; empty when unsubscribed from all.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn list_for_user(pool: &SqlitePool, user_id: i64) -> Result<Vec<i64>, AppError> {
    let ids = sqlx::query_scalar::<_, i64>(
        "SELECT match_id FROM match_notifications WHERE user_id = ? ORDER BY match_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(ids)
}

/// Saves the user's two per-channel alert preferences (`users.notify_email` /
/// `users.notify_telegram`); values are booleans, both may be muted.
///
/// # Errors
/// `AppError::Sqlx` on query failure.
pub async fn update_settings(
    pool: &SqlitePool,
    user_id: i64,
    email_enabled: bool,
    telegram_enabled: bool,
) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET notify_email = ?, notify_telegram = ? WHERE id = ?")
        .bind(email_enabled)
        .bind(telegram_enabled)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Mirrors one already-rendered Telegram alert to the admin's broadcast
/// channel when the admin is among `recipient_ids`, using the admin's own
/// dedicated bot token.
///
/// The mirror is best-effort: a missing channel/token, a DB/crypto error or a
/// failed send only logs a warning and never affects the private per-user
/// notifications. It deliberately ignores `notify_telegram` — the admin's
/// subscriptions are the only trigger.
///
/// # Returns
/// `true` when a copy was actually sent to the channel.
async fn mirror_to_broadcast(
    state: &AppState,
    recipient_ids: &[i64],
    text: &str,
    html: bool,
) -> bool {
    // Don't proceed further is there is no recipient
    if recipient_ids.is_empty() {
        return false;
    }
    // Get targets from telegram configuration, exit if there is no target
    let target = match telegram::broadcast_target(&state.pool).await {
        Ok(Some(target)) => target,
        Ok(None) => return false,
        Err(e) => {
            tracing::warn!("failed to load telegram broadcast channel: {e}");
            return false;
        }
    };

    // Exit if recipient ids don't contains the admin id
    if !recipient_ids.contains(&target.admin_user_id) {
        return false;
    }

    // Check that telegram targets has a bot token
    let Some(cipher) = &target.bot_token_cipher else {
        tracing::warn!("broadcast channel has no bot token configured");
        return false;
    };

    // get broadcast key to decipher the bot token
    let Some(key) = state.broadcast_bot_key.as_ref() else {
        tracing::warn!("broadcast bot token cannot be decrypted without SECRET_KEY");
        return false;
    };

    // decrypt the bot token
    let token = match decrypt(key, cipher) {
        Ok(token) => token,
        Err(e) => {
            tracing::warn!("failed to decrypt the broadcast bot token: {e}");
            return false;
        }
    };

    // Create a new telegram client here for the message sending.
    // We do that because the bot token may change in the future.
    let bot = state.broadcast_bot_factory.create(&token);
    // Send html message is available or default one either
    let started = Instant::now();
    let sent = if html {
        bot.send_message_html(&target.chat_id, text).await
    } else {
        bot.send_message(&target.chat_id, text).await
    };
    let elapsed_ms = started.elapsed().as_millis();

    // Check the sent response to trace and have a return
    match sent {
        Ok(()) => {
            tracing::info!(
                chat_id = %target.chat_id,
                elapsed_ms,
                "alert mirrored to broadcast channel"
            );
            true
        }
        Err(e) => {
            tracing::warn!(
                elapsed_ms,
                error = %e,
                "failed to mirror alert to the telegram broadcast channel"
            );
            false
        }
    }
}

/// Fans out one notification round for the changes produced by a calendar
/// sync. Only `Change::Scheduled` flips are handled (Tbd → Scheduled with the
/// final kickoff); best-effort per message — send failures are logged, never
/// propagated, so one dead channel cannot skip the others.
///
/// # Arguments
/// * `state` — shared dependencies (pool, mailer, telegram).
/// * `changes` — changes from a single `calendar.sync()` call; consumed.
pub async fn process(state: &AppState, changes: Vec<Change>) {
    // if there is no change, we don't do anything here
    for change in changes {
        // we only care about scheduled matches
        let Change::Scheduled {
            id,
            team,
            opponent,
            kickoff,
        } = change;
        // process the match
        process_one(state, id, team.to_string(), opponent.to_string(), kickoff).await;
    }
}

/// Notifies all subscribers of one scheduled match, then deletes their
/// subscription rows (single-fire semantics: a match change is announced once).
///
/// Per subscriber: email when `notify_email`, Telegram when `notify_telegram`
/// AND a linked chat id exists (unlinked users simply get nothing on that
/// channel). Send/DB failures warn and continue.
///
/// # Arguments
/// * `match_id` — LNR id identifying `match_notifications` rows.
async fn process_one(
    state: &AppState,
    match_id: u32,
    team: String,
    opponent: String,
    kickoff: String,
) {
    // get the subscribers for this match id
    let subscribers: Vec<(i64, String, Option<String>, bool, bool)> = match sqlx::query_as(
        "SELECT mn.user_id, u.email, u.telegram_id, u.notify_email, u.notify_telegram \
         FROM match_notifications mn JOIN users u ON u.id = mn.user_id WHERE mn.match_id = ?",
    )
    .bind(i64::from(match_id))
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("failed to load subscribers for match {match_id}: {e}");
            return;
        }
    };

    // if there is no subscribers, we don't do anything here
    if subscribers.is_empty() {
        return;
    }

    // send the alert to all the subscribers
    let alert = MatchAlert {
        team,
        opponent,
        kickoff,
    };
    let text = format!(
        "Match scheduled: {} vs {} on {}.",
        alert.team, alert.opponent, alert.kickoff
    );
    let recipient_ids: Vec<i64> = subscribers.iter().map(|(user_id, ..)| *user_id).collect();
    // for each subscriber, notify through the enabled channels
    for (user_id, email, telegram_id, notify_email, notify_telegram) in &subscribers {
        // If noifify email is enabled, send the email
        if *notify_email {
            if let Err(e) = state.mailer.send_match_alert(email, &alert).await {
                tracing::warn!("failed to send match alert email for user {user_id}: {e}");
            }
        }
        // If notify telegram is enabled, send the telegram message
        if *notify_telegram {
            // Check that the user has a telegram id
            if let Some(chat_id) = telegram_id {
                if let Err(e) = state.telegram.send_message(chat_id, &text).await {
                    tracing::warn!(
                        "failed to send match alert on telegram for user {user_id}: {e}"
                    );
                }
            }
        }
    }
    mirror_to_broadcast(state, &recipient_ids, &text, false).await;
    // clear the notifications for this match in the database
    if let Err(e) = sqlx::query("DELETE FROM match_notifications WHERE match_id = ?")
        .bind(i64::from(match_id))
        .execute(&state.pool)
        .await
    {
        tracing::warn!("failed to clear notifications for match {match_id}: {e}");
    }
}

// ============================== SEATS ALERTS ==============================

/// Alerts the `notify`-kind subscribers of one match about its newly-seen
/// resale seats.
///
/// The recipients come from [`crate::services::tickets::pending_requesters`]
/// (one row per explicit alert subscription — `pending` + `kind = 'notify'`,
/// contact info + channel gates included): email when `notify_email`,
/// Telegram when `notify_telegram` and a linked chat id exists. Best-effort
/// per channel — a failed send is warned and never blocks the other
/// subscribers or channels. Nothing is sent when nobody holds an alert
/// subscription for the match. Buying kinds are silently excluded by the
/// query itself.
///
/// # Arguments
/// * `match_id` — LNR id selecting the `ticket_requests` rows.
/// * `alert` — the alert content (opponent, date, the new seats only).
pub async fn notify_new_seats(state: &AppState, match_id: u32, alert: &SeatAlert) {
    let started = Instant::now();
    // Get the alert subscribers (notify kind) for the match.
    let requesters = match tickets::pending_requesters(&state.pool, i64::from(match_id)).await {
        Ok(requesters) => requesters,
        Err(e) => {
            tracing::warn!("failed to load alert subscribers for match {match_id}: {e}");
            return;
        }
    };
    if requesters.is_empty() {
        return;
    }

    // Client-rendered Telegram message (HTML parse mode): header, divider,
    // clickable CTA and one bullet per seat, in the house design.
    let text = alert.telegram_text();
    let recipient_ids: Vec<i64> = requesters.iter().map(|r| r.user_id).collect();
    let (mut emailed, mut messaged) = (0u32, 0u32);
    for requester in requesters {
        // If notify_email is enabled, send the seats alert email
        if requester.notify_email {
            match state.mailer.send_seats_alert(&requester.email, alert).await {
                Ok(()) => {
                    emailed += 1;
                    tracing::info!("seats alert email sent for user {}", requester.user_id);
                }
                Err(e) => tracing::warn!(
                    "failed to send seats alert email for user {}: {e}",
                    requester.user_id
                ),
            }
        }
        // If notify_telegram is enabled, send the Telegram message
        if requester.notify_telegram {
            // An unlinked Telegram account has no chat id to message
            if let Some(chat_id) = &requester.telegram_id {
                match state.telegram.send_message_html(chat_id, &text).await {
                    Ok(()) => {
                        messaged += 1;
                        tracing::info!(
                            "seats alert telegram message sent for user {}",
                            requester.user_id
                        );
                    }
                    Err(e) => tracing::warn!(
                        "failed to send seats alert on telegram for user {}: {e}",
                        requester.user_id
                    ),
                }
            }
        }
    }
    let broadcast = mirror_to_broadcast(state, &recipient_ids, &text, true).await;
    tracing::info!(
        match_id,
        emailed,
        messaged,
        broadcast,
        elapsed_ms = started.elapsed().as_millis(),
        "seats alert processed for notify subscribers"
    );
}

// ============================== CART ALERTS ==============================

/// Tells one user that seats were added to their shop cart or that
/// the wave ended on a failure with what is already in the cart.
///
/// One recipient only (the checkout acts for a single request): email when
/// `notify_email`, Telegram when `notify_telegram` and a linked chat id
/// exists. Best-effort per channel — a failed send is warned, never
/// propagated (the cart itself is already filled; the logs keep the trace).
///
/// # Arguments
/// * `user_id` — owner of the request and of the cart.
/// * `alert` — wave content (added/claimed/total progress, cart link, failure
///   flag).
pub async fn notify_cart_ready(state: &AppState, user_id: i64, alert: &CartAlert) {
    let started = Instant::now();
    let contact: Option<(String, Option<String>, bool, bool)> = match sqlx::query_as(
        "SELECT email, telegram_id, notify_email, notify_telegram FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(row) => row,
        Err(e) => {
            tracing::warn!("failed to load cart-alert recipient {user_id}: {e}");
            return;
        }
    };
    let Some((email, telegram_id, notify_email, notify_telegram)) = contact else {
        tracing::warn!("cart-alert recipient {user_id} no longer exists");
        return;
    };

    let text = alert.telegram_text();
    let (mut emailed, mut messaged) = (false, false);
    if notify_email {
        match state.mailer.send_cart_alert(&email, alert).await {
            Ok(()) => emailed = true,
            Err(e) => tracing::warn!("failed to send cart alert email for user {user_id}: {e}"),
        }
    }
    if notify_telegram {
        if let Some(chat_id) = &telegram_id {
            match state.telegram.send_message_html(chat_id, &text).await {
                Ok(()) => messaged = true,
                Err(e) => {
                    tracing::warn!("failed to send cart alert on telegram for user {user_id}: {e}")
                }
            }
        }
    }
    let broadcast = mirror_to_broadcast(state, &[user_id], &text, true).await;
    tracing::info!(
        user_id,
        added = alert.added,
        claimed = alert.claimed,
        total = alert.total,
        failed = alert.failed,
        emailed,
        messaged,
        broadcast,
        elapsed_ms = started.elapsed().as_millis(),
        "cart alert processed"
    );
}
