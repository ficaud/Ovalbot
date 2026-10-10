# OvalBot — Phase 2 : Calendrier des matchs

> [!info] Objectif
> Une page calendrier listant les **matchs à domicile** à venir, mise à jour
> automatiquement depuis un flux **iCalendar (`.ics`)**. Le noyau est un **crate local
> réutilisable** organisé en **couches** (ports & adapters), pensé pour être porté
> demain vers d'autres équipes/villes et exposé via une **API publique**.

## Règles métier

- **Matchs à domicile uniquement.** Le filtre domicile/extérieur est piloté par une
  configuration (nom d'équipe / stade), pas codé en dur.
- **Liste dynamique** : seuls les matchs **à venir** sont exposés (les matchs passés
  sont retirés ou marqués "terminé").
- **Multi-club** : le site permet de **switcher** entre plusieurs clubs (menu déroulant)
  pour consulter leurs matchs à domicile.
- **Mise à jour périodique** : le flux `.ics` est re-pollé à intervalle régulier ; les
  dates/heures passées de "TBD" à une date précise mettent la page à jour.
- **Fin de saison** : quand il n'y a plus de match à venir, message « bonnes vacances,
  on revient vite ! ».
- La page authentifiée reste **réservée aux comptes `active`** (règle héritée de la Phase 1).
- L'API publique est **en lecture seule** et n'expose que les matchs **à venir**.

## Source de données

- **Flux iCalendar (`.ics`)** comme source de vérité (norme RFC 5545), à la place du
  scraping HTML. Chaque match = un `VEVENT` (`DTSTART`, `SUMMARY`, `LOCATION`, `UID`).
- **URL du flux à déterminer** (voir « À clarifier »). rugbyfixture ne fournit pas
  forcément de `.ics` ; le site du club ou un agrégateur peut en fournir un.

> [!todo] À CLARIFIER — URL du feed `.ics`
> Trouver une URL stable du calendrier iCal du Stade Rochelais (billetterie.staderochelais.com,
> site du club, ou agrégateur).

> [!todo] À CLARIFIER — Représentation du "TBD"
> Un `.ics` a toujours une date. Comment un match "date à déterminer" apparaît-il ?
> (date placeholder, mention dans `SUMMARY`, ou match absent du flux ?). Impacte le diff Phase 3.

> [!todo] À CLARIFIER — Timezone
> Les feeds sont souvent en UTC avec `TZID=Europe/Paris`. Le parseur doit la gérer.

## Architecture en couches

Pattern **ports & adapters** : chaque couche ne dépend que de celle du dessous, et
uniquement via des **traits** (jamais des implémentations concrètes).

```
┌─────────────────────────────────────────────────────────────┐
│ 5. PRÉSENTATION  (app ovalbot)                              │
│    Axum : routes auth + API publique   Svelte : page + menu │
├─────────────────────────────────────────────────────────────┤
│ 3. APPLICATION  (app ovalbot)                               │
│    CalendarService : sync() · list_upcoming() · list_teams()│
│    Orchestration pure, aucune dépendance HTTP/SQL           │
├─────────────────────────────────────────────────────────────┤
│ 2. PORTS (traits)                                           │
│    EventParser · ScheduleFetcher · MatchRepository          │
├─────────────────────────────────────────────────────────────┤
│ 1. DOMAINE  (crate calendar, PUR, aucune I/O)               │
│    Event · Match · MatchStatus · Team · classify/upcoming/diff│
├─────────────────────────────────────────────────────────────┤
│ 4. ADAPTERS  (app ovalbot)                                  │
│    IcsParser · HtmlParser · HttpFetcher(reqwest) · SqliteStore│
└─────────────────────────────────────────────────────────────┘
```

### 1. Domaine — crate `calendar` (pur, portable)

- **Aucune I/O, aucun framework, aucune sérialisation.**
- Types :
  - `Event { uid, summary, location, dtstart, dtend }` (neutre, issu du parse).
  - `Match { team, opponent, kickoff, status, is_home }`.
  - `MatchStatus { Tbd, Scheduled, Finished }` (enum fermé, matching exhaustif).
  - `Team { id, name, home_venue }`.
- Logique pure :
  - `classify(events, &Team) -> Vec<Match>` (classification domicile/extérieur + statut).
  - `upcoming(&[Match], now) -> Vec<Match>`.
  - `diff(old, new) -> Vec<Change>` (transition TBD → Scheduled, réutilisé en Phase 3).
- **Parseur** : trait `EventParser { fn parse(&self, raw) -> Result<Vec<Event>, ParseError> }`
  + implémentation **générique** `IcsParser`.

### 2. Ports (traits)

Contrats exigés par l'application :

- `EventParser` : `raw` (texte) → `Vec<Event>` (défini dans le crate, produit des types domaine).
- `ScheduleFetcher` : récupère le contenu brut d'une URL (HTTP).
- `MatchRepository` : persistance (`upsert`, `list_upcoming`, `list_teams`).

### 3. Application — `CalendarService`

- Facade unique injectée dans `AppState`, **neutre vis-à-vis de l'UI** :
  - `sync(source)` : fetch → parse → classify → diff → persist.
  - `list_upcoming(team)` : lecture depuis le cache SQLite.
  - `list_teams()` : liste des clubs configurés.
- Ne dépend que des trois traits, jamais de `reqwest`/`sqlx`/Axum.

### 4. Adaptateurs (implémentent les traits)

- `IcsParser` (générique, dans le crate).
- `HtmlParser` (spécifique au site, **dans l'app**) : même trait, ajoutable sans rien changer.
- `HttpFetcher` (reqwest) : télécharge le feed.
- `SqliteMatchStore` (sqlx) : lecture/écriture de la table `matches`.
- `Noop*` : versions factices pour les tests.

### 5. Présentation

- Routes Axum (authentifiée + publique) appelant `CalendarService`.
- Sérialisation en `MatchView` (DTO, reste dans l'app, **pas dans le crate**).
- Frontend Svelte : page interactive + menu déroulant des clubs.

## Registre des clubs (config statique)

La liste des clubs est **configurée via `.env`** (pas de table `teams`). Ajouter un club =
ajouter une entrée JSON, sans toucher au code. La migration vers une table SQL restera
possible sans impact sur le domaine ni les parseurs.

```env
CALENDAR_SYNC_INTERVAL_SECS=3600
CALENDAR_TEAMS=[{"id":"stade-rochelais","name":"Stade Rochelais","home_venue":"Stade Marcel-Deflandre","source_url":"https://...","source_format":"ics"}]
```

| Variable | Description |
|---|---|
| `CALENDAR_SYNC_INTERVAL_SECS` | Intervalle de re-poll du feed (secondes) |
| `CALENDAR_TEAMS` | JSON listant `{ id, name, home_venue, source_url, source_format }` |

## Flux de données

**Sync (arrière-plan, scheduler tokio) :**

```
scheduler ──▶ CalendarService.sync()
                ├─ ScheduleFetcher.fetch(url) ─▶ HttpFetcher ─▶ réseau (.ics)
                ├─ EventParser.parse(raw) ─────▶ IcsParser ──▶ Vec<Event>
                ├─ classify(events, team) ─────▶ Vec<Match>
                ├─ diff(old, new) ─────────────▶ Vec<Change>   (Phase 3)
                └─ MatchRepository.upsert(...) ─▶ SqliteStore ─▶ SQLite
```

**Lecture (requête HTTP) :**

```
GET /api/public/teams/{team}/home-matches
        └─▶ CalendarService.list_upcoming(team)
                └─ MatchRepository.list_upcoming(...) ─▶ SqliteStore ─▶ SQLite
        └─▶ Json<Vec<MatchView>>
```

## Modèle de données

Migration `0004_create_matches.sql` :

```sql
CREATE TABLE matches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team TEXT NOT NULL,
    opponent TEXT NOT NULL,
    kickoff TEXT NOT NULL,
    status TEXT NOT NULL,
    source_uid TEXT,
    created_at TEXT NOT NULL
);
```

| Champ | Type | Contraintes | Description |
|---|---|---|---|
| `id` | INTEGER | PK, auto-increment | Identifiant |
| `team` | TEXT | NOT NULL | `id` du club (`Team.id`) |
| `opponent` | TEXT | NOT NULL | Adversaire |
| `kickoff` | TEXT | NOT NULL | Date/heure du match (RFC 3339) |
| `status` | TEXT | NOT NULL | `tbd` \| `scheduled` \| `finished` |
| `source_uid` | TEXT | NULL | `UID` du `VEVENT` (dédup) |
| `created_at` | TEXT | NOT NULL | Date d'insertion |

## Stack & infra

| Élément | Choix |
|---|---|
| Parseur iCal | crate **`ical`** (établi) ou **`ics`** (pur Rust) — à trancher selon gestion `TZID`/`RRULE` |
| HTTP | **reqwest** (déjà présent, rustls) |
| Scheduler | **tokio** (`tokio::spawn` + `interval`) |
| Crate | **local au workspace** (`calendar`), publication différée |
| Persistance | **cache SQLite** via **sqlx** (migration `0004`) + **sync périodique** |
| Config clubs | **`.env`** (`CALENDAR_TEAMS` JSON) |

## Endpoints API

| Méthode | Route | Accès | Description |
|---|---|---|---|
| `GET` | `/api/matches` | connecté (`active`) | Matchs à domicile à venir, triés par date |
| `GET` | `/api/public/teams` | public | Liste `{ id, name }` des clubs (pour le dropdown) |
| `GET` | `/api/public/teams/{team}/home-matches` | public | Matchs **à venir** du club `{team}`, réponse `[{ opponent, kickoff, status }]` |

> Les routes publique et authentifiée partagent le **même `CalendarService`** ; seule la
> garde (`AuthUser`) diffère.

## UI / charte

- La home (`+page.svelte`) affiche la liste des matchs (adversaire, date/heure) + menu
  déroulant pour switcher de club.
- Message de fin de saison quand la liste est vide.
- Même charte que la Phase 1 (jaune `#FFD500` / noir, mobile-first, anglais).

## Qualité & tests

- Tests unitaires du crate `calendar` : parsing `.ics`, classification domicile/extérieur,
  filtrage `upcoming()`, `diff()` (transition TBD → Scheduled).
- `CalendarService` testé avec des doubles (`NoopFetcher`, repo in-memory) sans réseau ni DB.
- `cargo test` + clippy `-D warnings` verts.

## Hors périmètre (reporté)

- Alertes e-mail de programmation des matchs (Phase 3).
- Bot & commandes (Phase 4).
- Scraping HTML (remplacé par le flux `.ics`, mais le trait `EventParser` l'autorise si besoin).
- Édition dynamique des clubs via admin (config statique pour l'instant).
