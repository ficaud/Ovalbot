# OvalBot — Cahier des charges

> [!info] Objectif
> Offrir une méthode fiable pour obtenir des places pour les matchs du Stade Rochelais à domicile, via un bot de revente + un site web simple et épuré (2 pages : calendrier + dashboard).

## Feuille de route (découpage en phases)

| Phase | Contenu | Priorité |
|---|---|---|
| **Phase 1** | Site d'inscription (10 places max) + liste d'attente + connexion | Immédiate |
| **Phase 2** | Page calendrier des matchs (liste dynamique) | Immédiate |
| **Phase 3** | Alertes e-mail quand une date/heure de match est annoncée | Immédiate |
| **Phase 4** | Intégration du bot + commandes + dashboard | À la toute fin |

> [!warning] Le bot n'est pas intégré au départ
> Le bot existe déjà mais ne sera **pas** incorporé dans un premier temps. Toutes les fonctionnalités qui en dépendent (tranches de prix, détection de commande validée, mise en panier automatique, notification "commande prête") sont reportées en Phase 4. Voir la section dédiée plus bas.

---

## Phase 1 — Inscriptions & liste d'attente

**Besoin** : limiter le nombre de personnes pouvant accéder au site pour éviter la saturation des bots à l'avenir.

**Réponse** :
- Page de création de compte avec liste d'attente (max 10 inscriptions actives).
- Connexion au compte → accès à une page simple avec la liste des matchs à domicile.

**Critères d'acceptation** :
- Au-delà de 10 comptes actifs, les nouveaux inscrits sont placés en liste d'attente.
- Un utilisateur en attente ne peut pas accéder au contenu (dashboard/calendrier).
- La connexion est protégée (session/cookie + mot de passe haché).

> [!todo] À CLARIFIER — Passage "attente" → "actif"
> Comment un utilisateur en attente devient-il actif ? Validation manuelle par l'admin, ou automatique quand une place se libère ? À définir.

> [!todo] À CLARIFIER — Modèle de données
> Lister les entités et leurs relations (User, Waitlist, Match, Commande, Notification, ...). Une section dédiée est prévue plus bas.

---

## Phase 2 — Calendrier des matchs

**Besoin** : une page qui se met à jour au fil de la saison (suppression des matchs terminés, MAJ des dates/heures).

**Réponse** :
- Consultation du calendrier rugbyfixture (`https://www.rugbyfixture.com/team/stade-rochelais`) pour les dates/heures.
- Liste dynamique des matchs, avec suppression des matchs terminés.
- Affichage sympathique en fin de saison : « bonnes vacances, on revient vite ! ».

**Critères d'acceptation** :
- Un match passé disparaît de la page (ou est marqué "terminé").
- Quand une date passe de "TBD" à une date précise, la page est mise à jour.
- Le message de fin de saison s'affiche quand il n'y a plus de match à venir.

---

## Phase 3 — Alertes de programmation

**Besoin** : être notifié quand une date/heure précise d'un match est annoncée (pour savoir si je peux y aller).

**Réponse** :
- Notification par e-mail quand une date/heure est annoncée.
- Déclencheur : détection du passage "TBD" → date précise (un diff, pas un mail à chaque scraping).

> [!todo] RAPPEL — Service d'e-mail
> Explorer le service de mail à mettre en place, notamment une solution **auto-hébergée sur Raspberry Pi** (ex : SMTP local, `lettre`, ou un relay type Resend/Postmark). À trancher.

> [!todo] RAPPEL — Hébergement
> Définir où tournent le site (backend + frontend + BDD) et sur quelle machine (Raspberry Pi ?). À trancher.

---

## Phase 4 — Bot & commandes (à la toute fin)

> [!warning] Reporté
> Cette section est reportée. Elle sera détaillée au moment de l'intégration du bot.

**Besoins couverts (rappel)** :
- Pouvoir réaliser une commande de place pour un match (fenêtre au clic → commande enregistrée → consultable dans le dashboard → supprimable, pas modifiable).
- Types de commande : solo (tranche de prix précise), duo (tranche de prix), groupe (places pelouse).
- Être notifié quand une commande est validée mais nécessite une action (mail "passe à l'action", ~20 min avant que le panier soit vidé).

> [!todo] À CLARIFIER — Tranches de prix
> Définir les valeurs exactes des tranches (solo / duo / groupe). Reporté en même temps que le bot.

> [!todo] À CLARIFIER — Sécurité & légal
> Idée 1 (l'utilisateur valide son propre compte, on ne stocke jamais son mot de passe billetterie) vs Idée 2 (mot de passe en clair par le robot — risque RGPD). Recommandation : Idée 1. À redéfinir plus tard.

---

## Contraintes & solutions

| Contrainte | Solution envisagée | Statut |
|---|---|---|
| Détecter la validation d'une commande | *(vide)* | À CLARIFIER (Phase 4) |
| Envoyer un e-mail | Service mail exploitable par le backend | À CLARIFIER (Phase 3) |
| Trouver dates/heures des matchs | rugbyfixture.com | OK |
| Liste d'attente empêchant la création de compte | *(vide)* | À CLARIFIER (Phase 1) |
| Identité visuelle | billetterie.staderochelais.com/fr | OK |
| Logo/icône | pièce jointe | OK |
| Technologies | Rust (backend) + Svelte (frontend) | OK |
| Compatible mobile | Design responsive / mobile-first obligatoire | OK |
| Langue | Site entièrement en anglais dans un premier temps (UI, code, commentaires) | OK |

> [!todo] RAPPEL — Idée complémentaire
> Envisager **Gluetun** comme VPN pour le bot (déjà noté dans tes idées).

---

## Outils de développement

| Outil | Description | Statut |
|---|---|---|
| Devcontainer | Conteneur minimal (Ubuntu 24.04 + Rust/cargo + Node + SQLite) pour un environnement reproductible | OK |
| Repo `ovalbot-devenv` | Repo séparé (cloné en `my-env/`, gitignoré) : image overlay + configs perso (nvim LazyVim, lazygit, zsh) | OK |
| nvim | LazyVim, config bind-montée depuis `~/.config/nvim` | OK |
| lazygit | Config bind-montée depuis `~/.config/lazygit` | OK |

> [!info] Pattern (repris de relic-core)
> Le devcontainer du projet fournit uniquement l'outillage de base (cargo, Node, SQLite). Les outils perso (nvim, lazygit, zsh) sont ajoutés par une **image overlay** bâtie depuis `my-env/` (repo `ovalbot-devenv`), qui **bind-monte** les configs du host (`~/.config/nvim`, `~/.config/lazygit`, `~/.gitconfig`, `~/.zshrc`).

---

## Qualité & tests

- **Tests unitaires obligatoires** : chaque fonction métier réalisée (auth, règles de liste d'attente, hachage, sessions, endpoints) doit être couverte par des tests unitaires (`cargo test`).
- La CI exécute `cargo test` sur chaque branche et chaque PR ; un build non vert bloque le merge.
- Linting strict : `cargo fmt --check` + `cargo clippy -- -D warnings`.

## CI/CD & déploiement

| Étape | Déclencheur | Contenu | Déploie |
|---|---|---|---|
| `ci.yml` | `push` toutes branches + `pull_request` | format, clippy, tests unitaires, build backend/frontend | Non |
| `deploy.yml` | tag `v*` sur `main` + `workflow_dispatch` | build image Docker multi-arch (arm64) → push GHCR | Oui |

- **Modèle trunk-based** : `main` est protégée (PR + checks verts obligatoires). Les branches de fonctionnalité ne déploient jamais ; seule `main` (via un tag) publie une image.
- **Versionnement SemVer** : source de vérité = `version` dans `Cargo.toml`. Un tag `vX.Y.Z` publie l'image `ghcr.io/ficaud/ovalbot:X.Y.Z` (+ `latest`, `:<sha>`).
- **Registre** : GitHub Container Registry (GHCR).
- **Rollback** : re-pointer le Pi vers un tag d'image précédent.

## Version affichée

- Sur **tout le site**, la version d'OvalBot est affichée de façon **discrète**, en **transparence** sur un côté (coin d'écran).
- Source : `version` du `Cargo.toml`, exposée par l'endpoint `GET /api/version`, récupérée par le frontend.

## Synthèse des rappels / à clarifier

- [x] Modèle de données (entités & relations) — voir `phase1.md`
- [ ] Tranches de prix (Phase 4)
- [ ] Sécurité & légal : Idée 1 vs Idée 2 (Phase 4)
- [ ] Service e-mail (explorer auto-hébergement Raspberry Pi)
- [x] Hébergement & SGBD — Raspberry Pi 4/5 (arm64) + SQLite
- [x] Passage "attente" → "actif" — activation auto à la libération d'une place + manuelle (admin)
- [ ] Parcours utilisateur end-to-end à détailler
