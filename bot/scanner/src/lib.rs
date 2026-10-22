//! Thin async wrapper around the parser crate: the billetterie passes the
//! bot needs — home discovery (`discover`) and direct seat polling
//! (`fetch_seats`). OvalBot owns persistence, matching and notifications.

pub mod app;
