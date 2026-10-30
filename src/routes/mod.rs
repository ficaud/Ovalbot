//! HTTP layer: one module per resource; handlers are thin and delegate the
//! business work to [`crate::services`].

pub mod admin;
pub mod auth;
pub mod calendar;
pub mod notifications;
pub mod shop;
pub mod telegram;
pub mod tickets;
