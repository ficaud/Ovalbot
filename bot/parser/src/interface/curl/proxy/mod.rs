//! Proxy pool management: normalized lines, round-robin rotation, failover.

mod proxy_api;
mod proxy_core;

pub use proxy_api::{
    PROXY_ENABLED, PROXY_MANAGER, ProxyMode, retry_with_proxy_mode, set_proxy_enabled,
};
pub use proxy_core::ProxyManager;
