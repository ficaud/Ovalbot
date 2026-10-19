//! Thin async wrapper around the parser crate: one scan of the billetterie
//! → `Vec<Encounter>` (OvalBot owns dedup, matching and notifications).

pub mod app;
