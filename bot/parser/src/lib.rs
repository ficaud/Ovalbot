//! TELESCRAP parser crate (imported): fetches a club's billetterie pages and
//! extracts resale encounters + seats. Layered as `core` (types), `interface`
//! (I/O + top-level API), `app` (per-club parsers), `controller` (fetch trait).

pub mod app;
pub mod controller;
pub mod core;
pub mod interface;
