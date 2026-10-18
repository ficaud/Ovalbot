use curl::easy::Easy;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub(crate) struct Proxy {
    pub(crate) url: String,
}

/// Manages a pool of proxy URLs loaded from a file or fetched from the
/// ProxyScrape API, with round-robin rotation and automatic failover.
///
/// Priority order:
/// 1. If `PROXIES_FILE` env var is set → load proxies from that file
/// 2. Otherwise → fetch from the ProxyScrape API
///
/// Each call to `get()` returns the next proxy in the rotation. When a
/// proxy fails, it is removed from the available pool. Use `reset_all()`
/// to re-fetch a fresh list from the API and restart the rotation.
pub struct ProxyManager {
    pub(crate) all: Mutex<Vec<Proxy>>,
    pub(crate) available: Mutex<Vec<Proxy>>,
    pub(crate) cursor: Mutex<usize>,
}

impl ProxyManager {
    /// Creates a new `ProxyManager` with empty proxy lists and a cursor set to 0.
    /// The proxy list will be lazily loaded on first use.
    ///
    /// # Return
    /// A new instance of `ProxyManager` ready to manage proxy URLs.
    pub(crate) fn new() -> Self {
        ProxyManager {
            all: Mutex::new(vec![]),
            available: Mutex::new(vec![]),
            cursor: Mutex::new(0),
        }
    }

    /// Fetches the proxy list from a file (`PROXIES_FILE` env) or from
    /// the ProxyScrape API (called once on first use).
    fn ensure_fetched(&self) {
        let mut all = self.all.lock().unwrap();
        if !all.is_empty() {
            return;
        }

        // Priority 1: load from file if PROXIES_FILE is set
        if let Ok(path) = std::env::var("PROXIES_FILE") {
            match Self::fetch_proxies_from_file(&path) {
                Ok(proxies) if !proxies.is_empty() => {
                    let count = proxies.len();
                    *all = proxies.clone();
                    let start = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as usize
                        % count;
                    *self.cursor.lock().unwrap() = start;
                    *self.available.lock().unwrap() = proxies;
                    log::info!("[PROXY] {} proxy(ies) loaded from {}", count, path);
                    return;
                }
                Ok(_) => log::warn!("[PROXY] File {} is empty", path),
                Err(e) => log::error!("[PROXY] Failed to read {}: {}", path, e),
            }
        }

        // Priority 2: fetch from ProxyScrape API
        match Self::fetch_proxies_from_api() {
            Ok(proxies) => {
                let count = proxies.len();
                *all = proxies.clone();
                let start = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as usize
                    % count;
                *self.cursor.lock().unwrap() = start;
                *self.available.lock().unwrap() = proxies;
                log::info!("[PROXY] {} proxy(ies) loaded from ProxyScrape API", count);
            }
            Err(e) => {
                log::error!("[PROXY] Failed to fetch proxy list: {e}");
            }
        }
    }

    /// Performs a synchronous GET request to the ProxyScrape API and
    /// parses the response into a list of proxy URLs.
    ///
    /// # Arguments
    /// * `url` - The URL of the ProxyScrape API endpoint to fetch proxies
    ///
    /// # Return
    /// A `Result` containing a vector of `Proxy` instances on success, or an error if the request or parsing fails.
    fn fetch_proxies_from_api() -> Result<Vec<Proxy>, Box<dyn std::error::Error>> {
        let url = "https://api.proxyscrape.com/v4/free-proxy-list/get?request=display_proxies&proxy_format=protocolipport&format=text&protocol=http&country=fr";

        let mut easy = Easy::new();
        easy.url(url)?;
        easy.follow_location(true)?;
        // Identify ourselves to avoid being blocked
        let mut headers = curl::easy::List::new();
        headers.append("User-Agent: Telescrap/1.0")?;
        easy.http_headers(headers)?;

        let mut response = Vec::new();
        {
            let mut transfer = easy.transfer();
            transfer.write_function(|data| {
                response.extend_from_slice(data);
                Ok(data.len())
            })?;
            transfer.perform()?;
        }

        let text = String::from_utf8(response)?;
        let proxies: Vec<Proxy> = text
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|line| Proxy {
                url: line.to_string(),
            })
            .collect();

        if proxies.is_empty() {
            log::warn!("[PROXY] ProxyScrape API returned an empty list");
        }

        Ok(proxies)
    }

    /// Reads proxies from a file. Supports two formats per line:
    /// - Full URL: `http://user:pass@ip:port`
    /// - Short form: `ip:port` or `ip:port:user:pass` (automatically
    ///   prefixed with `http://`)
    ///
    /// # Arguments
    /// * `path` - The file path to read proxy URLs from
    ///
    /// # Return
    /// A `Result` containing a vector of `Proxy` instances on success, or an error if the file cannot be read or contains malformed lines.
    fn fetch_proxies_from_file(path: &str) -> Result<Vec<Proxy>, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let proxies: Vec<Proxy> = content
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| Proxy {
                url: Self::normalize_proxy_url(line),
            })
            .collect();
        Ok(proxies)
    }

    /// Converts a single proxy line to a full URL.
    ///
    /// Supports:
    /// - `http://user:pass@1.2.3.4:8080` → passed through
    /// - `1.2.3.4:8080` → `http://1.2.3.4:8080`
    /// - `1.2.3.4:8080:user:pass` → `http://user:pass@1.2.3.4:8080`
    fn normalize_proxy_url(line: &str) -> String {
        if line.contains("://") {
            return line.to_string();
        }

        let parts: Vec<&str> = line.split(':').collect();
        match parts.len() {
            // ip:port
            2 => format!("http://{}:{}", parts[0], parts[1]),
            // ip:port:user:pass
            4 => format!("http://{}:{}@{}:{}", parts[2], parts[3], parts[0], parts[1]),
            _ => {
                log::warn!("[PROXY] Skipping malformed proxy line: {}", line);
                String::new()
            }
        }
    }

    /// Returns the next proxy URL in the rotation, or `None` if the
    /// pool is empty.
    ///
    /// # Return
    /// An `Option<String>` containing the next proxy URL if available, or `None` if the proxy pool is empty.
    pub fn get(&self) -> Option<String> {
        self.ensure_fetched();
        let available = self.available.lock().unwrap();
        if available.is_empty() {
            return None;
        }
        let mut cursor = self.cursor.lock().unwrap();
        // Ensure cursor is within bounds
        if *cursor >= available.len() {
            *cursor = 0;
        }
        let idx = *cursor;
        *cursor = (*cursor + 1) % available.len();
        Some(available[idx].url.clone())
    }

    /// Marks a proxy URL as failed, removing it from the available pool.
    ///
    /// # Arguments
    /// * `url` - The proxy URL to mark as failed and remove from the available pool
    pub fn mark_failed(&self, url: &str) {
        self.ensure_fetched();
        let mut available = self.available.lock().unwrap();
        let before = available.len();
        available.retain(|p| p.url != url);
        let removed = before - available.len();
        if removed > 0 {
            // Adjust cursor so we don't skip a proxy
            let mut cursor = self.cursor.lock().unwrap();
            if *cursor > 0 {
                *cursor = cursor.saturating_sub(1);
            }
            log::info!(
                "[PROXY] Removed {} — {} proxy(ies) remaining",
                url,
                available.len()
            );
        }
    }

    /// Re-fetches the proxy list from the API and restarts the
    /// rotation. Call this when all proxies have been exhausted.
    pub fn reset_all(&self) {
        let mut all = self.all.lock().unwrap();
        all.clear();
        self.ensure_fetched();
    }

    /// Number of currently available (non-failed) proxies.
    ///
    /// # Return
    /// The count of currently available proxies in the pool.
    pub fn available_count(&self) -> usize {
        self.ensure_fetched();
        self.available.lock().unwrap().len()
    }

    /// Total number of proxies fetched (including failed ones).
    ///
    /// # Return
    /// The total count of proxies that have been fetched and are being managed,
    /// including those that have been marked as failed.
    pub fn total_count(&self) -> usize {
        self.ensure_fetched();
        self.all.lock().unwrap().len()
    }

    /// Returns the current proxy URL without advancing the cursor.
    /// The same proxy will be returned on the next call until `advance()`
    /// is called (useful for sticky proxy mode).
    pub fn peek(&self) -> Option<String> {
        self.ensure_fetched();
        let available = self.available.lock().unwrap();
        if available.is_empty() {
            return None;
        }
        let cursor = self.cursor.lock().unwrap();
        let idx = if *cursor >= available.len() {
            0
        } else {
            *cursor
        };
        Some(available[idx].url.clone())
    }

    /// Advances the cursor to the next proxy in the rotation.
    /// Use after marking a proxy as failed in sticky mode.
    pub fn advance(&self) {
        self.ensure_fetched();
        let available = self.available.lock().unwrap();
        if available.is_empty() {
            return;
        }
        let mut cursor = self.cursor.lock().unwrap();
        *cursor = (*cursor + 1) % available.len();
    }
}

// To run these tests:
// cargo test -p parser --lib proxy_core -- --nocapture
//
#[cfg(test)]
mod tests {
    use super::*;
    use crate::interface::curl::proxy::proxy_api::{
        PROXY_ENABLED, ProxyMode, retry_with_proxy_mode, set_proxy_enabled,
    };

    /// Helper to create a ProxyManager pre-populated with test proxies.
    fn make_manager(urls: &[&str]) -> ProxyManager {
        let proxies: Vec<Proxy> = urls.iter().map(|u| Proxy { url: u.to_string() }).collect();
        ProxyManager {
            all: Mutex::new(proxies.clone()),
            available: Mutex::new(proxies),
            cursor: Mutex::new(0),
        }
    }

    // -----------------------------------------------------------------------
    // normalize_proxy_url
    // -----------------------------------------------------------------------

    #[test]
    fn normalize_full_url_passthrough() {
        let url = "http://user:pass@1.2.3.4:8080";
        assert_eq!(ProxyManager::normalize_proxy_url(url), url);
    }

    #[test]
    fn normalize_ip_port() {
        assert_eq!(
            ProxyManager::normalize_proxy_url("1.2.3.4:8080"),
            "http://1.2.3.4:8080"
        );
    }

    #[test]
    fn normalize_ip_port_user_pass() {
        assert_eq!(
            ProxyManager::normalize_proxy_url("1.2.3.4:8080:alice:secret"),
            "http://alice:secret@1.2.3.4:8080"
        );
    }

    #[test]
    fn normalize_malformed_returns_empty() {
        let result = ProxyManager::normalize_proxy_url("just-a-string");
        assert!(result.is_empty());
    }

    #[test]
    fn normalize_already_full_url() {
        let url = "socks5://user:pass@5.6.7.8:1080";
        assert_eq!(ProxyManager::normalize_proxy_url(url), url);
    }

    // -----------------------------------------------------------------------
    // get() - round-robin rotation
    // -----------------------------------------------------------------------

    #[test]
    fn get_returns_none_after_all_marked_failed() {
        let m = make_manager(&["http://a:1"]);
        m.mark_failed("http://a:1");
        assert_eq!(m.get(), None);
    }

    #[test]
    fn get_rotates_through_all_proxies() {
        let m = make_manager(&["http://a:1", "http://b:2", "http://c:3"]);
        assert_eq!(m.get(), Some("http://a:1".into()));
        assert_eq!(m.get(), Some("http://b:2".into()));
        assert_eq!(m.get(), Some("http://c:3".into()));
        // wraps around
        assert_eq!(m.get(), Some("http://a:1".into()));
    }

    #[test]
    fn get_single_proxy_repeats() {
        let m = make_manager(&["http://only:1"]);
        for _ in 0..5 {
            assert_eq!(m.get(), Some("http://only:1".into()));
        }
    }

    // -----------------------------------------------------------------------
    // mark_failed()
    // -----------------------------------------------------------------------

    #[test]
    fn mark_failed_removes_proxy_from_rotation() {
        let m = make_manager(&["http://a:1", "http://b:2", "http://c:3"]);
        assert_eq!(m.get(), Some("http://a:1".into()));
        assert_eq!(m.get(), Some("http://b:2".into()));
        m.mark_failed("http://b:2");
        assert_eq!(m.get(), Some("http://c:3".into()));
        assert_eq!(m.get(), Some("http://a:1".into()));
        for _ in 0..10 {
            assert_ne!(m.get(), Some("http://b:2".into()));
        }
    }

    #[test]
    fn mark_failed_nonexistent_is_noop() {
        let m = make_manager(&["http://a:1"]);
        m.mark_failed("http://ghost:9");
        assert_eq!(m.available_count(), 1);
        assert_eq!(m.get(), Some("http://a:1".into()));
    }

    #[test]
    fn mark_all_failed_exhausts_pool() {
        let m = make_manager(&["http://a:1", "http://b:2"]);
        m.mark_failed("http://a:1");
        m.mark_failed("http://b:2");
        assert_eq!(m.available_count(), 0);
        assert_eq!(m.get(), None);
    }

    // -----------------------------------------------------------------------
    // available_count / total_count
    // -----------------------------------------------------------------------

    #[test]
    fn counts_after_failures() {
        let m = make_manager(&["http://a:1", "http://b:2", "http://c:3"]);
        assert_eq!(m.total_count(), 3);
        assert_eq!(m.available_count(), 3);
        m.mark_failed("http://b:2");
        assert_eq!(m.total_count(), 3);
        assert_eq!(m.available_count(), 2);
    }

    // -----------------------------------------------------------------------
    // set_proxy_enabled / PROXY_ENABLED flag
    // -----------------------------------------------------------------------
    #[test]
    fn set_proxy_enabled_toggles_flag() {
        set_proxy_enabled(false);
        assert!(!PROXY_ENABLED.load(std::sync::atomic::Ordering::Relaxed));
        set_proxy_enabled(true);
        assert!(PROXY_ENABLED.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn retry_with_proxy_disabled_mode_skips_proxy() {
        let result = retry_with_proxy_mode(
            |proxy| {
                assert!(proxy.is_none(), "Expected no proxy in Disabled mode");
                Ok::<_, Box<dyn std::error::Error>>(42)
            },
            ProxyMode::Disabled,
        );
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn retry_with_proxy_sticky_uses_same_proxy_on_success() {
        // In sticky mode, the same proxy should be returned on consecutive calls
        // when no failure occurs. We can verify peeks don't advance the cursor.
        let m = make_manager(&["http://sticky:1", "http://next:2"]);
        assert_eq!(m.peek(), Some("http://sticky:1".into()));
        assert_eq!(m.peek(), Some("http://sticky:1".into()));
        assert_eq!(m.peek(), Some("http://sticky:1".into()));
        // Only advance() should move the cursor
        m.advance();
        assert_eq!(m.peek(), Some("http://next:2".into()));
    }

    #[test]
    fn retry_with_proxy_failover_disabled_mode() {
        set_proxy_enabled(false);
        let result = retry_with_proxy_mode(
            |proxy| {
                assert!(proxy.is_none(), "Expected no proxy when globally disabled");
                Ok::<_, Box<dyn std::error::Error>>(42)
            },
            ProxyMode::Rotating,
        );
        assert_eq!(result.unwrap(), 42);
        set_proxy_enabled(true);
    }
}
