# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-11-XX

### Added

- `bot/parser` : logging checkout using dupral extraction.
- `common/crypto` : new modal for user registration that will get stored
  using AES encryption.
- `bot/purchase` : matching parsed seats and requests from user to
  trigger the PlanCheckout.
- `bot/parser/test` : real case pelouse seat composition for testing
  purpose. This is to improve the overall bot testing and operating
  in real case scenario.
- `services/bot/checkout` : move PlanCheckout to real add to cart
  operation using user credential.
- `bot` : new parameter that enables full checkout exchange capture
  to get more context whether there is an issue or not in the add to
  cart process.
- `bot/parser/test` : regression tests to avoid falling into the prefix
  error again.
- `mailer/cart_reader` : CTA now points to the match resale ticket link
  so that the user only needs to connect itself to pursue the purchase.
- `services/tickets` : user actions on a closed request from the ticket
  request modal — confirm the purchase (`accepted → purchased`) and retry
  (`accepted|failed → pending`, releasing the seats the bot had claimed so
  the matcher can serve the request again). New routes

### Fixed

- `README` : add sudo to command for creating the data base folder.
- `bot/services/checkout` : more robust checkout operation by better
  management of the seat state.
- `bot/parser` : use html extractor to detect seat gone in addition
  of the missing machinery that is required to perform a dupral request.
- `bot/services/checkout` : Checkout out ticket in packs instead. Tickets
  are sold in pack and not in single id format. So we need to get the pack
  id to communication with the resale website.
- `bot/parser` : navigates durpal settings as a json to avoid getting MissingContext
  error.
- `bot/parser` : fix wrong prefix that produced a false link.
- `bot/purchase` : First serve the seat that just appears to avoid
  trying purchase one from a list that has been taken beforehand.

### Changed

- `bot checkout` : les notifications « places au panier » (e-mail, Telegram,
  texte brut) pointent désormais leur **bouton principal vers la page du match**
  (la page de revente du match pris, `CheckoutTarget.resale_link`) ; le lien
  panier devient un lien secondaire, et l'alerte d'échec cite les deux.

- `bot checkout` : la revente vend des **packs atomiques**. L'unité d'achat
  devient le pack : un seul POST par pack avec tous ses billets
  (`selected_tickets_{pack}=t1,t2`), et le matcher ne planifie que des packs
  entiers tenant dans le quota restant — un pack plus gros que le quota est
  ignoré (aucun dépassement de commande).
- `bot checkout` : une file d'attente, une erreur réseau ou un formulaire
  expiré laissent désormais la demande `pending` (nouvelle tentative à la
  vague suivante) au lieu de la marquer `failed`. Seuls un login refusé et un
  contexte d'ajout au panier disparu restent terminaux.
- `bot checkout` : le dry run journalise la présence des identifiants boutique
  (`credentials=present|missing|unreadable`) sans se connecter.
- `bot checkout` : un `MissingContext` (page rendue en BigPipe sans la
  machinerie d'ajout au panier) laisse désormais la demande `pending` au lieu
  de la clore `failed` — seul un login boutique refusé reste terminal.
- `main` : avertissement au démarrage quand `CART_ENABLED` est actif sans aucun
  canal de notification réel (Resend et Telegram désactivés).

### Fixed

- `bot/parser` : les fichiers cookie jar des connexions échouées ne sont plus
  laissés dans le répertoire temporaire.
- `bot/parser` : une page de revente épuisée (plus de listing ni de slider de
  prix) est désormais lue comme `SeatGone` (nouvelle tentative à la vague
  suivante) et non plus comme un formulaire manquant (échec définitif).
- `bot/parser` : le verdict d'ajout au panier vient de la réponse AJAX
  (`added-to-cart` / `is_already_in_cart` / `refreshCartTimeout`) ; la page
  disponible étant rendue en BigPipe, l'absence de listing ou de slider dans le
  HTML brut n'est plus interprétée comme un échec.
- `bot/parser` : avec `CHECKOUT_DUMP_DIR`, chaque tentative d'ajout écrit la
  page reçue, la réponse AJAX et un fichier de métadonnées (pack, tickets,
  bornes du slider, issue) — succès compris.
- `bot/parser` : extraction du contexte d'ajout tolérante aux pages BigPipe —
  `drupal-settings-json` lu sans structure typée (la page authentifiée réelle
  n'a **pas** de carte `ajax`), endpoint AJAX reconstruit depuis l'URL produit
  quand `edit-add-to-cart` est absent, et diagnostic (`settings=`,
  `ajax_entry=`, `form_build_id=`, `slider=`) loggé quand le contexte reste
  introuvable. La capture live du 01/11 (DENAIN, session authentifiée) sert de
  fixture de régression.
- `bot/parser` : l'endpoint AJAX reconstruit (URL produit déjà absolue) n'est
  plus préfixé une seconde fois par la base — l'hôte devenait
  `…staderochelais.comhttps` et le POST échouait en `Network` (run live du
  01/11 22:27). `absolute_url` ne préfixe que les chemins relatifs, et le
  `Referer` suit la même règle.

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


