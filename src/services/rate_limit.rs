//! In-memory sliding-window rate limiting for the public auth endpoints
//! (`/api/register`, `/api/login`) — a brute-force / account-creation-abuse
//! guard for a site exposed to the internet.
//!
//! The limiter keeps, per client key (the client IP), the timestamps of the
//! events inside each policy's window. The two policies differ in what they
//! count:
//!
//! - `login` ([`Policy::LOGIN`]) counts **refused credentials only**, and a
//!   successful login clears the key's login events, so a legitimate user who
//!   mistypes once is never locked out;
//! - `register` ([`Policy::REGISTER`]) counts **every attempt** — creating
//!   accounts is the abuse itself, whatever the outcome.
//!
//! Denied requests are not recorded: hammering an already-blocked key does
//! not extend its lockout. State lives in memory only (a restart resets it),
//! which is acceptable for a brute-force guard.
//!
//! # Clock
//!
//! The wall clock is injected ([`RateLimiter::with_now`]) so tests can slide
//! the window without sleeping; production uses the monotonic
//! [`Instant::now`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Which quota a policy counts into — the slot the events land in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Refused credentials ([`Policy::LOGIN`]).
    Login,
    /// Registration attempts ([`Policy::REGISTER`]).
    Register,
}

/// One limiting policy: how many events within how long a window.
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// Slot the events count into (login / register).
    pub kind: Kind,
    /// Maximum admitted events inside one [`Policy::window`].
    pub limit: u32,
    /// Window length; only the events younger than this count.
    pub window: Duration,
}

impl Policy {
    /// Login: 5 refused credentials per 15 minutes per client.
    pub const LOGIN: Policy = Policy {
        kind: Kind::Login,
        limit: 5,
        window: Duration::from_secs(15 * 60),
    };

    /// Register: 3 registration attempts per hour per client.
    pub const REGISTER: Policy = Policy {
        kind: Kind::Register,
        limit: 3,
        window: Duration::from_secs(60 * 60),
    };
}

/// Wall-clock source, injectable so tests can slide the window without
/// sleeping.
type Now = Arc<dyn Fn() -> Instant + Send + Sync>;

/// Per-policy event slots of one client key — the two policies are isolated
/// (a key's login failures never count toward its register quota).
#[derive(Default)]
struct Slots {
    /// Events counted toward [`Policy::LOGIN`].
    login: Vec<Instant>,
    /// Events counted toward [`Policy::REGISTER`].
    register: Vec<Instant>,
}

/// Sliding-window event counters keyed by client key (IP string).
type Events = HashMap<String, Slots>;

/// In-memory rate limiter: read-only gate + explicit event recording, so a
/// route chooses what counts (all attempts for register, failures only for
/// login).
pub struct RateLimiter {
    events: Mutex<Events>,
    now: Now,
}

impl RateLimiter {
    /// A limiter reading the real monotonic clock.
    ///
    /// # Returns
    /// A fresh, empty [`RateLimiter`].
    pub fn new() -> Self {
        Self::with_now(Arc::new(Instant::now))
    }

    /// A limiter driven by a custom clock (tests only).
    ///
    /// # Arguments
    /// * `now` — called on every check/record to read the current instant.
    ///
    /// # Returns
    /// A fresh, empty [`RateLimiter`] reading `now`.
    pub fn with_now(now: Now) -> Self {
        Self {
            events: Mutex::new(HashMap::new()),
            now,
        }
    }

    /// Read-only gate: `Err(retry_after)` in seconds when `key` has already
    /// used its `policy` quota inside the window (the caller answers 429 with
    /// that delay), `Ok(())` otherwise. Nothing is recorded — recording is
    /// the caller's decision ([`record`](Self::record) /
    /// [`admit`](Self::admit)).
    ///
    /// # Arguments
    /// * `key` — client key (IP string) the events are tracked under.
    /// * `policy` — quota + window to enforce.
    ///
    /// # Returns
    /// `Ok(())` when allowed, else the number of seconds the caller should
    /// wait before retrying (never 0).
    pub fn check(&self, key: &str, policy: Policy) -> Result<(), u64> {
        let mut events = self.events.lock().unwrap();
        let now = (self.now)();
        let slot = Self::prune(&mut events, key, policy, now);
        if slot.len() >= policy.limit as usize {
            // Sliding window: the lockout ends when the oldest in-window
            // event leaves it (the slot is non-empty at this point by
            // construction).
            let oldest = slot.iter().min().copied().unwrap_or(now);
            let retry_after = policy
                .window
                .checked_sub(now.saturating_duration_since(oldest))
                .unwrap_or(Duration::ZERO);
            return Err(retry_after.as_secs().max(1));
        }
        Ok(())
    }

    /// Records one event for `key` under `policy`.
    ///
    /// # Arguments
    /// * `key` — client key (IP string) the event is tracked under.
    /// * `policy` — quota + window the event counts toward.
    ///
    /// # Panics
    /// Never (the mutex cannot be poisoned: no closure panics while held).
    pub fn record(&self, key: &str, policy: Policy) {
        let mut events = self.events.lock().unwrap();
        let slot = Self::prune(&mut events, key, policy, (self.now)());
        slot.push((self.now)());
    }

    /// Convenience gate for policies counting **every attempt**:
    /// [`check`](Self::check) then [`record`](Self::record) when allowed.
    ///
    /// # Arguments
    /// * `key` — client key (IP string) the events are tracked under.
    /// * `policy` — quota + window to enforce.
    ///
    /// # Returns
    /// Same contract as [`check`](Self::check); the event is recorded only
    /// when the call returns `Ok(())`.
    pub fn admit(&self, key: &str, policy: Policy) -> Result<(), u64> {
        self.check(key, policy)?;
        self.record(key, policy);
        Ok(())
    }

    /// Clears every event of `key` under `policy` — used by the login route
    /// after a successful authentication, so successes never contribute to a
    /// future lockout.
    ///
    /// # Arguments
    /// * `key` — client key (IP string) to clear.
    /// * `policy` — policy whose events are dropped.
    ///
    /// # Panics
    /// Never (same as [`record`](Self::record)).
    pub fn clear(&self, key: &str, policy: Policy) {
        self.events
            .lock()
            .unwrap()
            .entry(key.to_owned())
            .or_default()
            .slot_mut(policy)
            .clear();
    }

    /// Drops every stale event and every key left empty — bounds the memory
    /// of keys that never come back (per-touch pruning only frees a key when
    /// it is seen again). Called periodically by [`Self::spawn_sweeper`].
    pub fn sweep(&self) {
        let mut events = self.events.lock().unwrap();
        let now = (self.now)();
        events.retain(|_, slots| {
            slots
                .login
                .retain(|t| now.saturating_duration_since(*t) <= Policy::LOGIN.window);
            slots
                .register
                .retain(|t| now.saturating_duration_since(*t) <= Policy::REGISTER.window);
            !(slots.login.is_empty() && slots.register.is_empty())
        });
    }

    /// Number of client keys currently tracked (diagnostics/tests only).
    ///
    /// # Returns
    /// The count of keys, whatever the staleness of their events.
    pub fn tracked_keys(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    /// Spawns the background sweeper: drops stale events every 10 minutes so
    /// abandoned keys do not accumulate.
    ///
    /// # Arguments
    /// * `limiter` — the shared limiter to sweep.
    ///
    /// # Returns
    /// Nothing; the task runs for the process lifetime.
    pub fn spawn_sweeper(limiter: Arc<RateLimiter>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10 * 60));
            loop {
                interval.tick().await;
                limiter.sweep();
            }
        });
    }

    /// Prunes `key`'s stale events in place and returns its (possibly empty)
    /// slot; the map entry may stay empty until the sweeper removes it.
    fn prune<'a>(
        events: &'a mut Events,
        key: &str,
        policy: Policy,
        now: Instant,
    ) -> &'a mut Vec<Instant> {
        let slots = events.entry(key.to_owned()).or_default();
        let slot = slots.slot_mut(policy);
        slot.retain(|t| now.saturating_duration_since(*t) <= policy.window);
        slot
    }
}

impl Default for RateLimiter {
    /// Same as [`RateLimiter::new`] (the standard-construction convention).
    ///
    /// # Returns
    /// A fresh, empty [`RateLimiter`] reading the monotonic clock.
    fn default() -> Self {
        Self::new()
    }
}

impl Slots {
    /// The event slot a policy counts into.
    fn slot_mut(&mut self, policy: Policy) -> &mut Vec<Instant> {
        match policy.kind {
            Kind::Login => &mut self.login,
            Kind::Register => &mut self.register,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A limiter on a mutable fake clock; returns the limiter and an
    /// advance closure moving the clock forward.
    fn fake() -> (RateLimiter, Box<dyn FnMut(Duration) + Send>) {
        let current = Arc::new(Mutex::new(Instant::now()));
        let now = {
            let current = Arc::clone(&current);
            Arc::new(move || *current.lock().unwrap()) as Now
        };
        let advance = {
            let current = Arc::clone(&current);
            Box::new(move |d: Duration| *current.lock().unwrap() += d)
        };
        (RateLimiter::with_now(now), advance)
    }

    /// A policy small enough to exercise the window without long constants.
    fn tiny(kind: Kind, limit: u32) -> Policy {
        Policy {
            kind,
            limit,
            window: Duration::from_secs(60),
        }
    }

    // The gate admits up to the limit inside the window, then refuses with a
    // retry delay of at least 1 second; a denied attempt is not recorded, so
    // hammering never extends the lockout.
    #[test]
    fn admit_allows_up_to_the_limit_then_blocks() {
        let (limiter, mut advance) = fake();
        let policy = tiny(Kind::Register, 3);
        assert_eq!(limiter.admit("1.2.3.4", policy), Ok(()));
        assert_eq!(limiter.admit("1.2.3.4", policy), Ok(()));
        assert_eq!(limiter.admit("1.2.3.4", policy), Ok(()));
        let retry = limiter.admit("1.2.3.4", policy).unwrap_err();
        assert!((1..=60).contains(&retry));
        advance(Duration::from_secs(61));
        assert_eq!(limiter.admit("1.2.3.4", policy), Ok(()));
    }

    // The sliding window re-admits as the oldest events leave it.
    #[test]
    fn sliding_window_readmits_one_slot_at_a_time() {
        let (limiter, mut advance) = fake();
        let policy = tiny(Kind::Register, 2);
        limiter.admit("k", policy).unwrap();
        advance(Duration::from_secs(30));
        limiter.admit("k", policy).unwrap();
        assert!(limiter.admit("k", policy).is_err());
        advance(Duration::from_secs(31)); // the first event just expired
        assert_eq!(limiter.admit("k", policy), Ok(()));
    }

    // `check` is read-only: only `record` contributes events (the login
    // route's contract).
    #[test]
    fn check_never_records_an_event() {
        let (limiter, _) = fake();
        let policy = tiny(Kind::Login, 1);
        assert_eq!(limiter.check("k", policy), Ok(()));
        assert_eq!(limiter.check("k", policy), Ok(()));
        limiter.record("k", policy);
        assert!(limiter.check("k", policy).is_err());
    }

    // A successful login clears the key's events even at the limit.
    #[test]
    fn clear_resets_the_key_under_the_policy() {
        let (limiter, _) = fake();
        let policy = tiny(Kind::Login, 1);
        limiter.record("k", policy);
        assert!(limiter.check("k", policy).is_err());
        limiter.clear("k", policy);
        assert_eq!(limiter.check("k", policy), Ok(()));
    }

    // Different clients (and the two policies) never share their quotas.
    #[test]
    fn keys_and_policies_are_isolated() {
        let (limiter, _) = fake();
        // Same key: five login failures do not fill the register window.
        for _ in 0..Policy::LOGIN.limit {
            limiter.record("a", Policy::LOGIN);
        }
        assert!(limiter.check("a", Policy::LOGIN).is_err());
        assert!(limiter.check("a", Policy::REGISTER).is_ok());
        // Different keys never share their quota.
        for _ in 0..Policy::LOGIN.limit {
            limiter.record("b", Policy::LOGIN);
        }
        assert!(limiter.check("b", Policy::LOGIN).is_err());
    }

    // The sweeper drops stale events and empty keys from the map.
    #[test]
    fn sweep_removes_stale_events_and_empty_keys() {
        let (limiter, mut advance) = fake();
        limiter.record("gone", Policy::LOGIN);
        limiter.record("fresh", Policy::LOGIN);
        advance(Duration::from_secs(16 * 60));
        limiter.record("fresh", Policy::LOGIN);
        limiter.sweep();
        assert_eq!(limiter.tracked_keys(), 1);
        assert!(limiter.check("gone", Policy::LOGIN).is_ok());
    }

    // A key with only register events (empty login slot) survives a sweep as
    // long as its register window is fresh.
    #[test]
    fn sweep_keeps_a_key_with_only_fresh_register_events() {
        let (limiter, mut advance) = fake();
        limiter.record("only-register", Policy::REGISTER);
        limiter.sweep();
        assert_eq!(limiter.tracked_keys(), 1);
        advance(Duration::from_secs(61 * 60));
        limiter.sweep();
        assert_eq!(limiter.tracked_keys(), 0);
    }
}
