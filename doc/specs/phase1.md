# OvalBot — Phase 1 : Inscriptions & liste d'attente

> [!info] Objectif
> Site d'inscription (10 comptes actifs max) + liste d'attente + connexion + interface admin de gestion des inscriptions. Accès au contenu réservé aux comptes actifs.

## Règles métier

- **10 comptes actifs maximum.** Au-delà, les nouveaux inscrits sont placés en liste d'attente (`status = waiting`).
- La **liste d'attente est illimitée** et ne compte pas dans les 10 comptes actifs.
- **Activation automatique** : quand un compte actif est supprimé, le premier inscrit en attente passe `active`.
- **Identification** par un **email** (unique) et un **mot de passe**.
- **Suppression de compte réservée à l'admin** (l'utilisateur ne peut pas supprimer son propre compte).
- Un utilisateur `waiting` ne peut **pas** accéder au contenu (dashboard/calendrier).
- Le premier compte **admin** est créé via un **script CLI** dédié (`create_admin`).

## Modèle de données

Table `users` pour les comptes (pas de table séparée pour la liste d'attente), plus une table `sessions` pour l'authentification :

| Champ | Type | Contraintes | Description |
|---|---|---|---|
| `id` | INTEGER | PK, auto-increment | Identifiant |
| `email` | TEXT | UNIQUE, NOT NULL | Identifiant de connexion |
| `password_hash` | TEXT | NOT NULL | Mot de passe haché (Argon2) |
| `status` | TEXT | NOT NULL | `active` \| `waiting` |
| `role` | TEXT | NOT NULL | `admin` \| `user` |
| `created_at` | TEXT | NOT NULL | Date de création |
| `activated_at` | TEXT | NULL | Date de passage `active` |

Table `sessions` :

| Champ | Type | Contraintes | Description |
|---|---|---|---|
| `id` | TEXT | PK | Jeton de session opaque (aléatoire) |
| `user_id` | INTEGER | FK → `users.id`, NOT NULL | Compte lié |
| `created_at` | TEXT | NOT NULL | Date de création |
| `expires_at` | TEXT | NOT NULL | Date d'expiration (renouvelée à chaque activité) |

## Auth & sécurité

- **Argon2** pour le hachage des mots de passe.
- **Session côté serveur** : table `sessions` + cookie httpOnly. Révocation possible (suppression de la ligne).
  - **`session_id`** : jeton opaque aléatoire (≥ 32 octets).
  - **Cookie** : `session=<id>; HttpOnly; SameSite=Lax; Path=/` (+ `Secure` en production).
  - **Durée** : 30 jours, renouvelée à chaque activité.
  - **Déconnexion** : suppression de la ligne en base + cookie expiré (`Max-Age=0`).
  - **Expiration** : session invalide si absente ou expirée ; purge périodique des lignes expirées.
- Pas de rate limiting en Phase 1.
- Pas de validation par e-mail en Phase 1 (reporté).

## Stack & infra

| Élément | Choix |
|---|---|
| Backend | **Rust + Axum** (dernière version stable, 0.8.x) |
| Frontend | **SvelteKit** (Svelte 5) |
| Base de données | **SQLite** via **sqlx** (migrations embarquées) |
| Hachage mot de passe | **argon2** (variante **Argon2id**) |
| Hébergement | **Raspberry Pi 4/5 (arm64)** — serveur local, pas d'exposition publique au départ |
| Déploiement | **Docker Compose** (1 service `ovalbot` en Phase 1, multi-arch arm64, volume pour la base) |
| Servir le front | Build SvelteKit (`adapter-static`) servi par Axum |
| Cookies | **`axum-extra`** (feature `cookie`) — sessions serveur, jeton opaque |
| Migrations | **`sqlx::migrate!()`** embarquées au runtime + **`sqlx-cli`** (sqlite) en dev pour générer les fichiers |

> [!info] Déploiement
> Build **Docker multi-stage** (build SvelteKit → build Rust → image runtime minimale), image **arm64** via `buildx`. La base SQLite est persistée sur un **volume Docker**. Le Compose n'expose qu'un service `ovalbot` en Phase 1 ; les services `bot` + `gluetun` (VPN) viendront s'ajouter en Phase 4.

> [!note] Versions figées
> Axum est pris à la **dernière stable** (0.8.x). Attention : depuis 0.8, la syntaxe des path params a changé (`{id}` au lieu de `:id`).

## Parcours utilisateur

1. **Inscription** : email + mot de passe → si `< 10` actifs, `active` ; sinon `waiting`.
2. **Connexion** : email + mot de passe → cookie de session.
3. **Écran attente** : si `waiting`, affiché, sans accès au contenu.
4. **Accueil** : si `active`, accès à la page simple (placeholder en Phase 1, calendrier en Phase 2).
5. **Admin** : interface listant les comptes (statut, dates), activation manuelle possible, suppression.

## Endpoints API

| Méthode | Route | Accès | Description |
|---|---|---|---|
| `POST` | `/api/register` | public | Créer un compte (active ou waiting selon la place) |
| `POST` | `/api/login` | public | Connexion, crée la ligne `sessions` et pose le cookie httpOnly |
| `POST` | `/api/logout` | connecté | Déconnexion, supprime la session en base + expire le cookie |
| `GET` | `/api/me` | connecté | Infos du compte courant |
| `GET` | `/api/version` | public | Version de l'application (issue de `Cargo.toml`) |
| `GET` | `/api/admin/users` | admin | Liste des comptes |
| `POST` | `/api/admin/users/:id/activate` | admin | Activer manuellement un compte en attente |
| `DELETE` | `/api/admin/users/:id` | admin | Supprimer un compte (libère une place) |

> Le compte admin initial est créé via `cargo run --bin create_admin` (CLI), pas via l'API.

## Interface admin

- Liste des comptes : email, statut, rôle, `created_at`, `activated_at`.
- Actions : **activer** (manuel, en plus de l'activation auto), **supprimer**.
- La suppression d'un compte `active` déclenche la promotion automatique du premier `waiting`.

## UI / charte

- Couleurs du **Stade Rochelais** : jaune `#FFD500` / noir.
- Logo : `doc/assets/logo.png`.
- **Compatible mobile obligatoire** (design responsive / mobile-first).
- **Langue** : le site est **entièrement en anglais** dans un premier temps (même règle pour le code et les commentaires).
- **Version affichée** : sur tout le site, affichage discret de la version d'OvalBot, en transparence sur un côté (via `GET /api/version`).

## Qualité & tests

- **Tests unitaires obligatoires** pour chaque fonction métier réalisée (hachage, liste d'attente, activation auto, sessions, endpoints).
- La CI exécute `cargo test` sur chaque branche/PR ; build non vert = merge bloqué.

## Hors périmètre (reporté)

- Bot & commandes (Phase 4).
- Calendrier des matchs (Phase 2).
- Alertes e-mail (Phase 3).
- Rate limiting, validation par e-mail, sécurité avancée.
