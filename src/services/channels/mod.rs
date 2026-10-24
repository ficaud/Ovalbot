//! Notification channel transports: email ([`mailer`]), Telegram
//! ([`telegram`]) and the channel-neutral alert payloads they share
//! ([`alerts`]). Pure transports — the fan-out decisions live in
//! [`crate::services::notifications`].

pub mod alerts;
pub mod mailer;
pub mod telegram;
