use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

use super::proxy_core::ProxyManager;

/// Controls how proxies are selected during HTTP fetching.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProxyMode {
    /// No proxy — direct connection.
    Disabled,
    /// Round-robin rotation across the pool (default behavior).
    Rotating,
    /// Keep using the same proxy until it fails (sticky).
    Sticky,
}

/// Global proxy manager, initialised lazily on first use.  The proxy
/// list is fetched from the ProxyScrape API when the first request is
/// made.
pub static PROXY_MANAGER: LazyLock<ProxyManager> = LazyLock::new(ProxyManager::new);

/// Global flag to enable/disable proxy usage at runtime.
/// Set via `set_proxy_enabled()`. Defaults to `false` in OvalBot: direct connections
/// unless the proxy pool is explicitly enabled (e.g. `PROXY_ENABLED=true`).
pub static PROXY_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enables or disables proxy usage at runtime.
/// When disabled, `retry_with_proxy_mode` calls the closure with `None` (direct connection).
pub fn set_proxy_enabled(enabled: bool) {
    PROXY_ENABLED.store(enabled, Ordering::Relaxed);
    if enabled {
        log::info!("[PROXY] Proxy usage enabled");
    } else {
        log::info!("[PROXY] Proxy usage disabled — using direct connections");
    }
}

/// Retries a closure `f` with automatic proxy selection and failover.
///
/// # Arguments
/// * `f` - A closure that takes an `Option<&str>` representing the proxy URL (or `None` for direct connection)
/// * `proxy_mode` - How to select proxies when retrying
///
/// # Return
/// A `Result<T, Box<dyn std::error::Error>>` containing the successful result of `f` or an error if all proxies are exhausted.
pub fn retry_with_proxy_mode<F, T>(
    mut f: F,
    proxy_mode: ProxyMode,
) -> Result<T, Box<dyn std::error::Error>>
where
    F: FnMut(Option<&str>) -> Result<T, Box<dyn std::error::Error>>,
{
    // If proxy usage is disabled or the pool is empty, skip directly.
    if proxy_mode == ProxyMode::Disabled
        || !PROXY_ENABLED.load(Ordering::Relaxed)
        || PROXY_MANAGER.total_count() == 0
    {
        return f(None);
    }

    let mut last_err = None;
    while let Some(proxy_url) = match proxy_mode {
        ProxyMode::Sticky => PROXY_MANAGER.peek(),
        _ => PROXY_MANAGER.get(),
    } {
        // Extract and display ip:port from the proxy URL (e.g. "http://1.2.3.4:8080")
        let display = proxy_url.split("://").nth(1).unwrap_or(&proxy_url);
        log::info!("[PROXY] Using proxy: {}", display);

        match f(Some(&proxy_url)) {
            Ok(val) => return Ok(val),
            Err(e) => {
                PROXY_MANAGER.mark_failed(&proxy_url);
                if proxy_mode == ProxyMode::Sticky {
                    PROXY_MANAGER.advance();
                }
                last_err = Some(e);
                if PROXY_MANAGER.available_count() == 0 {
                    break;
                }
            }
        }
    }
    Err(last_err
        .unwrap_or_else(|| "All proxies exhausted and no direct connection configured".into()))
}
