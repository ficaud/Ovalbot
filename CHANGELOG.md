# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.2] - 2026-10-26

### Added

- `doc/SQL_HELPERS.md` : useful SQL queries to inspect the SQLite tables
  that list all the resale tickets that have been scanned.
- `cargo` : jiff to manage log timezones (Paris).
- `frontend/routes/` : legal notice page, privacy policy page and terms
  of service page.
- `ci/deploy.yml` : x86_64 image deployment.

### changed

- `frontend/footer` : Add link to the version that redirects user to the
  project's GitHub repository.
- `Telegram` : add `telegram_text` to create a better looking Telegram messages.

### Fixed

- `frontend/page.svelte` : Page title remains "Ovalbot" when user is on the
  main page. No more unwanted changes when user navigates elsewhere.

### Removed

- `frontend/parge.svelte` : Bot settings from admin panel because that doesn't
  make sense for the moment.

## [0.3.1] - 2026-10-24

### Fixed

- `test/` : .HTML compression to avoid getting too much .HTML code flooding the repo code
  language statistics.

## [0.3.0] - 2026-10-24

### Added

- `frontend/src/lib/TicketRequestModal.svelte`, `src/services/tickets.rs` : Kind `notify`
  for the ticket request modal and in the tikcets pending requesters list so that the
  bot only notifies the user without adding it to basket.
- `src/routes/tickets.rs` : ticket-request API — `PUT` / `DELETE /api/requests/{match_id}`
  to transmit and remove the user's request (single / duo / group with max price), and
  `GET /api/requests` to list the current user's requests.
- `migrations/0009_create_ticket_requests.sql`, `src/services/tickets.rs` : SQLite
  persistence for ticket requests.
- `bot/parser`, `bot/scanner` : import of the resale bot — scrapes the Stade Rochelais
  billetterie and exposes the found seats to the main crate.
- `src/services/matching.rs`, `calendar::DateWindow` : resolver mapping a scraped
  billetterie encounter to the calendar's stable LNR match id (title inference +
  date-window overlap, tolerating week-end ranges)
- `migrations/0010_create_ticket_offers.sql`, `src/services/notifications.rs` : new seats
  are recorded durably (no repeat on restart or later scans) and notified to users with a
  pending request on the match, via email and/or Telegram.
- `migrations/0011_create_resale_links.sql`, `src/services/resale.rs` : resale links
  discovered on the billetterie home page are persisted and polled directly.
- `doc/SQL_HELPERS.md` : gathered the useful SQL queries to inspect the SQLite database.

### Changed

- `services/` : refactor services into modules to make the repo easier to navigate.
- `frontend/src/lib/TicketRequestModal.svelte` : cleaner rewrite with exclusive modes
  (single / duo / group), and existing requests are now loaded at page startup so the
  modal is prefilled and locked requests cannot be modified.
- `frontend/src/routes/+page.svelte`, `src/routes/tickets.rs` : ticket requests are now
  only accepted for Stade Rochelais games.
- `src/services/alerts.rs` : alert payloads (`MatchAlert`, `SeatAlert`, seats listing)
  moved out of the mailer so email and Telegram share them.
- `templates/seats_alert.html`, `src/services/mailer.rs` : redesigned seats-alert email.
- `src/services/notifications.rs` : seats alerts are now sent on both channels.
- `bot/`, `calendar/`, `src/` : documentation campaign — rustdoc on every function,
  struct and module (including the imported bot crates).
- `.devcontainer/Dockerfile`, `Dockerfile` : update the Debian/Ubuntu base images.
- `.github/workflows/release.yml` : CI/CD release management on tags of the main branch.

### Fixed

- `templates/seats_alert.html`, `src/services/mailer.rs` : mobile seats-alert email
  now is responsive and doesn't break to the next line the price column.
- `src/services/matching.rs`, `bot/parser` : wrong match identification — a promotional
  badge on a match card ("DERNIÈRE MISE EN VENTE…") used to override the title, causing
  live rugby reservations to be skipped from the scan; badges no longer take precedence
  and non-match cards (repas, parking…) are rejected explicitly instead of being aliased
  to a nearby match.

### Removed

- `bot/` : The bot scanner of resale tickets doesn't includes proxies anymore.

## [0.2.0] - 2026-10-15

### Added
- `services/telegram.rs`, `router/telegram.rs` : Add Telegram webhook route and set notifier to send messages via a telegram bot.
- `.ignore` : Add ignore file for IDEs.
- `SECURITY.md` : Add security policy.
- `tests/router.rs`, `tests/mailer.rs` : Better mailer and router tests to improve overall coverage.
- `page.svelte` : robot modal for future usage to allow user to give the bot instructions.

### Changed
- `services/`, `common/` : Split services and common code into separate modules to make it easier to read and navigate into the project.
- `frontend/src/` : Refactor frontend main page layout by extracting the two modals in separates files.

### Fixed
- `vite.config.ts` : Listen to all the interface so that the host can connect to the server that is operating inside the devcontainer.

## [0.1.3] - 2026-10-12

### Fixed
- `calendar/lnr.rs`, `calendar/schedule.rs` : match alerts now key on the **LNR match id**
  (`feuille-de-match` link) instead of a positional index, so reordering a journée no
  longer notifies the wrong fixture. `diff` also verifies both events are the same match.
- `migrations/0005_clear_match_notifications.sql` : clears subscriptions stored with the
  old positional ids (test phase).

## [0.1.2] - 2026-10-11

### Fixed
- `auth.rs` : set cookie's secure attribute to true ([#5](https://github.com/ficaud/Ovalbot/security/code-scanning/5)).
- `calendar/lnr.rs` : fix fixtures `.html` captured files size for `lnr.rs` tests.

## [0.1.1] - 2026-10-11

### Added
- `LICENSE` : GPLv3.0 license.
- `README.md` : badges for CI/CD, deployment, and coverage.
- `codecov.yml` : CI now perform a coverage report.

### Changed
- `deploy.yml` :Image deployment now only takes the semver norm and latest in main branch (and for tags), there is also the '<branch-name>` name.
- `src/, calendar/src/` : More comments on everything to get better understanding of the code.

### Fixed
- `SvelteKit` : update to version (from dependabot)
    - Updates cookie from 0.6.0 to 2.0.1
    - Updates @sveltejs/adapter-static from 3.0.10 to 4.0.0
    - Updates @sveltejs/kit from 2.70.3 to 3.0.1
- `auth.rs` : hard-coded cryptographic ([#1](https://github.com/ficaud/Ovalbot/security/code-scanning/1), [#2](https://github.com/ficaud/Ovalbot/security/code-scanning/2), [#3](https://github.com/ficaud/Ovalbot/security/code-scanning/3), [#4]() and [#5](https://github.com/ficaud/Ovalbot/security/code-scanning/4)).

## [0.1.0] - 2026-10-11

### Added
- `frontend/` : SvelteKit static single-page app (Svelte 5, `adapter-static`) with
  login/register, home (active vs waitlist), admin panel (list/activate/delete users),
  match calendar and per-match notification toggle; typed API client in `src/lib/api.ts`.
- `migrations/` : SQLite migrations embedded at runtime with `sqlx::migrate!()`
  (`users`, `sessions`, `match_notifications`, welcome-email marker).
- `src/` : Axum backend — user accounts (register/login/logout/me), admin user
  management, waitlist promotion, welcome emails and match schedule alerts via Resend,
  and TOP14 calendar sync.
- `calendar/` : Rust library that scrapes TOP14 match date/time from the LNR website
  and diffs consecutive snapshots to emit `Scheduled` changes for alerting.
- `templates/` : HTML email templates (`welcome.html`, `match_alert.html`) loaded at
  startup (env `TEMPLATE_DIR`) with `{{var}}` placeholders, so the mailer can send
  styled emails that are easy to evolve without a rebuild.


