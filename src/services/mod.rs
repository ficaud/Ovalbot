//! Business + integration services: auth/waitlist, calendar cache, mailer,
//! Telegram, match notifications, ticket requests, the bot's resale link
//! registry and the bot scan loop.

pub mod auth;
pub mod bot;
pub mod calendar;
pub mod email;
pub mod mailer;
pub mod matching;
pub mod notifications;
pub mod resale;
pub mod state;
pub mod telegram;
pub mod tickets;
pub mod waitlist;
