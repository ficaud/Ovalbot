# OvalBot — Phase 2 : Calendrier des matchs

> [!info] Objectif
> Une page calendrier listant les **matchs à domicile** à venir, avec un menu pour
> **switcher d'équipe**. Les données viennent du **scraping HTML** de `top14.lnr.fr`
> (via un crate local `calendar` réutilisable), mises en cache en mémoire dans l'app
> et re-synchronisées périodiquement.

## Règles métier

- **Matchs à domicile** : `for_team(events, team, is_home = true)` filtre le 1er club
  (toujours le domicile dans le DOM du LNR).
- **Liste dynamique** : seuls les matchs **à venir** sont exposés (`upcoming()` retire
  les `Finished`).
- **Multi-club** : la liste des 14 équipes du Top 14 vient du type `TeamType` (enum
  fermé dans le crate), exposée via un menu déroulant.
- **Mise à jour périodique** : re-scraping toutes les 20 minutes ; les dates/heures
  passent de `Tbd` à `Scheduled` au fil de la saison.
- La page est **réservée aux comptes `active`** (extracteur serveur `ActiveUser`).
- L'API est **en lecture seule** et n'expose que les matchs **à venir**.

## Source de données

- **Scraping HTML** du calendrier officiel : `https://top14.lnr.fr/calendrier-et-resultats/{saison}/j{n}`.
- `LnrParser` boucle sur les journées `j1` → `j50` et s'arrête au premier **HTTP 404**.
- Saison codée en dur (`2026-2027`).
- Selectors DOM : `.calendar-results__line` (match), `.club-line__name` (clubs),
  `.match-line__time` (heure), `.calendar-results__fixture-date` (date),
  `.match-line__score` (match joué).

## Architecture

### Crate `calendar` (pur, portable, sans sérialisation)

| Fichier | Rôle |
|---|---|
| `parse/mod.rs` | trait `EventParser` (`parse` + `parse_all`) |
| `parse/lnr.rs` | `LnrParser { season }` : fetch + parsing HTML par journée |
| `fetcher.rs` | `WebPageFetcher` (reqwest `blocking` + rustls) + `FetchError` |
| `team.rs` | `TeamType` (14 équipes + `Unknown`), `Team { id, name, team_type }` |
| `event.rs` | `MatchStatus { Tbd, Scheduled, Finished }`, `Event { id, team, opponent, kickoff, status }` |
| `match_date.rs` | `MatchDate { day, month, hour, minute }` + `parse` (détecte TBD) |
| `schedule.rs` | fonctions pures `upcoming`, `for_team(events, team, is_home)`, `diff` |
| `error.rs` | `ParseError`, `FetchError` |

- `Event.id = (journée - 1) * 7 + index` → clé stable et ordre chronologique.
- `kickoff` reste une **string française** (`"samedi 10 octobre 14h30"`) — pas de timestamp.
- `TeamType` expose `all()`, `id()` (slug), `from_id()`, `from_name()`, `Display`.

### App `ovalbot`

- `CalendarService` : cache mémoire `RwLock<Vec<Event>>` + `sync()` (via
  `tokio::task::spawn_blocking`) + `list_teams()` + `home_matches(team)`.
- Sync initial **bloquant** au démarrage, puis `tokio::spawn` + `interval` (20 min).
- DTOs de sérialisation dans l'app (le crate reste serde-free) :
  - `TeamView { id, name }`
  - `MatchView { id, team, opponent, kickoff, status }`
- Extracteur `ActiveUser` (session valide + `status == active`).

## Flux de données

```
main.rs
  ├─ CalendarService.sync()  (démarrage, bloquant)
  │     └─ spawn_blocking → LnrParser::new("2026-2027").parse_all()
  │            └─ fetch j1..j50 (stop au 404) → parse_events → Vec<Event>
  └─ tokio::spawn(interval 20 min) → CalendarService.sync()

GET /api/teams                     ─▶ CalendarService.list_teams()
GET /api/teams/{team}/matches      ─▶ CalendarService.home_matches(team)
        └─ upcoming(events) → for_team(.., is_home=true) → Vec<MatchView>
```

## Endpoints API

| Méthode | Route | Accès | Description |
|---|---|---|---|
| `GET` | `/api/teams` | `active` | Liste `[{ id, name }]` des 14 clubs |
| `GET` | `/api/teams/{team}/matches` | `active` | Matchs **à domicile à venir** de `{team}`, réponse `[{ id, team, opponent, kickoff, status }]` |

## UI / charte

- La home (`+page.svelte`) : dropdown des équipes (défaut **Stade Rochelais**) +
  **une carte par match** (équipe vs adversaire, kickoff, badge `Scheduled`/`Tbd`).
- Cartes **cliquables** (`<button>`), prêtes pour une future modale d'interaction.
- Même charte que la Phase 1 (jaune `#FFD500` / noir, mobile-first, anglais).

## Stack & infra

| Élément | Choix |
|---|---|
| Parsing HTML | crate **`scraper`** (CSS selectors) |
| HTTP | **reqwest** (`blocking` + rustls, `User-Agent` navigateur) |
| Scheduler | **tokio** (`interval`, défaut 20 min) |
| Cache | **mémoire** (`RwLock<Vec<Event>>`), pas de table SQLite pour l'instant |
| Saison | codée en dur (`2026-2027`) |
| Docker | `COPY calendar ./calendar` (path-dep) + `.dockerignore` |

## Qualité & tests

- Crate `calendar` : parsing LNR (fixtures HTML), `TeamType` (`all`/`id`/`from_id`/`from_name`),
  `MatchDate`, `schedule` (`upcoming`/`for_team`/`diff`), fetcher (404).
- App `ovalbot` : `CalendarService` (`list_teams`, `home_matches`), `ActiveUser`.
- `cargo fmt` + `clippy -D warnings` + `cargo test` verts, `npm run build` + `svelte-check` verts.

## Hors périmètre (reporté)

- Alertes e-mail de programmation des matchs (`diff` Tbd → Scheduled réutilisé) — Phase 3.
- Bot & commandes — Phase 4.
- Modale d'interaction sur les cartes (les cartes sont déjà cliquables).
- Vrai timestamp (année) et tri date/heure : `kickoff` reste une string française.
