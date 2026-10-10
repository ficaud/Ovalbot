# OvalBot — Phase 3 : Alertes de programmation des matchs

> [!info] Objectif
> Permettre à un utilisateur `active` d'activer une **notification** sur un match
> encore **non programmé** (`Tbd`) et de recevoir un **e-mail** quand celui-ci passe à
> **`Scheduled`** (date/heure annoncée). Fournisseur : **Resend** (déjà en place depuis la
> Phase 1.5).

## Règles métier

- Un utilisateur `active` peut activer ou désactiver l'alerte **par match**, sur les
  **14 équipes** du Top 14 (périmètre identique au calendrier de la Phase 2).
- Seuls les matchs **`Tbd`** sont notifiables : pas de cloche sur un match déjà
  `scheduled` ou `finished` (plus rien à annoncer).
- Déclencheur : passage **`Tbd → Scheduled`** détecté par `calendar::diff()`
  (un diff entre deux snapshots, pas un e-mail à chaque scraping).
- **Anti-doublon** : garanti par le diff (le snapshot suivant a `old == Scheduled`) **et**
  par la **suppression de l'abonnement** après envoi.
- L'envoi est **best-effort** : un échec Resend (réseau, quota, clé invalide…) est
  **loggé**, n'impacte ni le sync du calendrier ni la suppression de l'abonnement.
- Le mail est en **anglais** (cohérent avec le site).

## Modèle de données

Migration `0004_create_match_notifications.sql` :

```sql
CREATE TABLE match_notifications (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    match_id INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (user_id, match_id)
);
```

| Champ | Type | Contraintes | Description |
|---|---|---|---|
| `user_id` | INTEGER | FK → `users.id`, `ON DELETE CASCADE`, PK | Compte abonné |
| `match_id` | INTEGER | PK | Id stable du match : `(journée - 1) * 7 + index` (crate `calendar`) |
| `created_at` | TEXT | NOT NULL | Date de souscription |

> [!note] Id de match
> `match_id` est l'id stable produit par le parser LNR (`Event.id`). Il est identique d'un
> scraping à l'autre tant que la saison ne change pas (`2026-2027` codée en dur).

## Architecture

### Détection des changements

- `CalendarService::sync()` est refactoré pour **capturer l'ancien snapshot** avant de
  re-parser, appliquer `calendar::diff(old, new)`, **écrire le nouveau** snapshot, puis
  **retourner `Vec<Change>`**.
- Le snapshot `events` n'est **réécrit qu'en cas de succès** du parsing : un sync qui échoue
  conserve l'ancien état, donc le diff suivant reste correct (pas de faux positifs).

### Envoi (`src/notifications.rs`)

- Nouveau module `notifications` : `process(&AppState, changes: Vec<Change>)`.
- Pour chaque `Change::Scheduled { id, .. }` :
  1. `SELECT` des abonnés (`user_id`, `email`) pour ce `match_id` ;
  2. envoi de l'alerte via le mailer (best-effort) ;
  3. `DELETE` des lignes `match_notifications` de ce `match_id` (anti-doublon).
- Appelé depuis `main.rs` après chaque sync périodique réussi.

### Mailer

- Ajout d'une méthode au trait `Mailer` :
  `fn send_match_alert(&self, email: &str, alert: &MatchAlert) -> Result<(), MailError>`.
- `MatchAlert { team, opponent, kickoff }` (strings prêtes à l'emploi, `TeamType::to_string()`).
- Implémentations mises à jour : `ResendMailer`, `NoopMailer` (+ mocks de test).
- Sujet/texte en placeholder à valider (même remarque que la Phase 1.5).

## Flux de données

```
main.rs
  └─ boucle périodique (20 min)
       ├─ CalendarService.sync()
       │     └─ old = read() → parse → diff(old, new) → write(new) → Vec<Change>
       └─ notifications::process(state, changes)
              └─ pour chaque Change::Scheduled { id }
                     ├─ SELECT abonnés du match id
                     ├─ mailer.send_match_alert(email, alert)   (best-effort)
                     └─ DELETE match_notifications WHERE match_id = id
```

## Endpoints API

Tous protégés par l'extracteur `ActiveUser` (compte `active` requis).

| Méthode | Route | Accès | Description |
|---|---|---|---|
| `GET` | `/api/notifications` | `active` | Liste des `match_id` suivis par l'utilisateur courant |
| `POST` | `/api/notifications/{match_id}` | `active` | Activer l'alerte sur ce match (idempotent) |
| `DELETE` | `/api/notifications/{match_id}` | `active` | Désactiver l'alerte sur ce match |

## UI / charte

- **Cloche** en SVG inline (pas de lib d'icônes), placée **en haut à droite** de chaque
  carte de match **`Tbd`**.
- Deux états :
  - **inactive** : cloche en contour (couleur `var(--muted)`) ;
  - **active** : cloche remplie (couleur `var(--yellow)`).
- Infobulle au survol via attribut natif `title` :
  - inactif → « Be notified when this match gets a date » ;
  - actif → « Turn off notifications for this match ».
- Accessibilité : `aria-label` + `aria-pressed`.
- La carte match passe de `<button>` à `<div>` (un `<button>` imbriqué serait invalide),
  la cloche devenant le seul élément interactif.

## Stack & infra

| Élément | Choix |
|---|---|
| Fournisseur e-mail | **Resend** (HTTP API, déjà intégré Phase 1.5) |
| Déclencheur | `tokio::time::interval` existant (20 min) + `calendar::diff` |
| Nouvelle dépendance | aucune |
| Persistance | table SQLite `match_notifications` |

## Qualité & tests

- Crate `calendar` : compléter les tests de `diff` avec les cas limites (match qui reste
  `Tbd`, match qui reste `Scheduled` → aucun `Change`).
- Module `notifications` :
  - abonnement **idempotent** (double `POST` = une seule ligne) ;
  - désabonnement d'un match non suivi = no-op ;
  - `sqlx::test` flux complet : un `Change::Scheduled` → **1 mail** envoyé aux abonnés
    concernés uniquement → lignes **supprimées** ;
  - échec du mailer → log, pas de panic, abonnement quand même retiré.
- `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test` verts.
- `npm run build` + `npm run check` verts.

## Hors périmètre (reporté)

- Bot & commandes (Phase 4).
- Notifications push web / in-app (navigateur).
- File d'envoi / retry (outbox).
- Vrai timestamp (année) et tri date/heure : `kickoff` reste une string française.
