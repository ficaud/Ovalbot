# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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


