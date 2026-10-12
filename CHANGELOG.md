# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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


