//! I/O adapters: HTTP client and the top-level match/seat API.

pub mod club_manager;
pub mod curl;
pub mod match_manager;

pub use match_manager::ScanError;
