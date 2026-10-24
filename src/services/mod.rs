//! Business + integration services: accounts (auth/waitlist/email type),
//! calendar cache, notification channels (mailer/Telegram/alerts), match
//! notifications, ticket requests, and the bot (scan loop, matching, resale
//! link registry).

pub mod auth;
pub mod bot;
pub mod calendar;
pub mod channels;
pub mod email;
pub mod matching;
pub mod notifications;
pub mod resale;
pub mod state;
pub mod tickets;
pub mod waitlist;
