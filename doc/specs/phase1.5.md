# OvalBot — Phase 1.5 : Service d'e-mail (bienvenue)

> [!info] Objectif
> Mettre en marche le service de mail : envoi d'un **mail de bienvenue** à chaque
> utilisateur au moment où il devient **actif**. Fournisseur : **Resend** (HTTP API).

## Règles métier

- Un **mail de bienvenue** est envoyé à **chaque** passage d'un compte à `active`, quel que soit le chemin :
  1. **Inscription directe** (`< 10` actifs) ;
  2. **Promotion automatique** (`promote_next_waiting`, libération d'une place) ;
  3. **Activation manuelle** par l'admin (`activate_user`).
- L'envoi est **best-effort** : si Resend échoue (réseau, quota, clé invalide…), l'activation
  réussit quand même, l'erreur est **loggée** et n'impacte pas la réponse HTTP.
- **Anti-doublon** : la colonne `welcome_email_sent_at` sur `users` évite de renvoyer le mail
  si une activation est rejouée sur un compte déjà actif.
- Le mail est en **anglais** (cohérent avec le site).
- Le compte **admin** créé via `create_admin` (déjà actif à la création) ne déclenche **pas** de mail.

## Modèle de données

Migration `0003_add_welcome_email_sent_at.sql` :

```sql
ALTER TABLE users ADD COLUMN welcome_email_sent_at TEXT;
```

Table `users` (complément) :

| Champ | Type | Contraintes | Description |
|---|---|---|---|
| `welcome_email_sent_at` | TEXT | NULL | Date d'envoi du mail de bienvenue (anti-doublon) |

## Intégration Resend

| Élément | Choix |
|---|---|
| Fournisseur | **Resend** (HTTP API) |
| Appel | `POST https://api.resend.com/emails` via **reqwest** |
| Authentification | Header `Authorization: Bearer <RESEND_API_KEY>` |
| Format | **HTML** + repli **texte brut** |
| Expéditeur | `RESEND_FROM` (placeholder, domaine à vérifier) |

Variables d'environnement :

| Variable | Description | Exemple |
|---|---|---|
| `RESEND_API_KEY` | Clé API Resend | `re_...` |
| `RESEND_FROM` | Expéditeur | `OvalBot <onboarding@resend.dev>` |
| `RESEND_ENABLED` | Active/désactive l'envoi (défaut `false` en dev) | `true` / `false` |

> [!warning] Domaine vérifié
> Resend n'envoie vers de vrais destinataires que depuis un **domaine vérifié**. En l'absence de
> domaine, utiliser le domaine **sandbox** `onboarding@resend.dev` (réservé aux tests).

## Architecture

- Nouveau module `mailer` : trait `Mailer { fn send_welcome(&self, email) -> Result<(), MailError>; }`
  - `ResendMailer` : appelle l'API HTTP.
  - `NoopMailer` : utilisé quand `RESEND_ENABLED != "true"` (dev/tests).
- `AppState` porte le mailer (clé + from + enabled).
- Déclenchement **après** la transition réussie en base, dans les 3 fonctions concernées.
  - `promote_next_waiting` est refactoré : SELECT du premier `waiting` → UPDATE → envoi (nécessaire pour connaître l'email).
- Envoi **asynchrone** (non bloquant pour la réponse HTTP).

## Parcours

1. Un compte passe `active` (un des 3 chemins).
2. La transition est écrite en base + `welcome_email_sent_at` renseigné.
3. Le mail de bienvenue part (best-effort, log en cas d'échec).

## Endpoints API

Aucun nouvel endpoint.

## Qualité & tests

- Trait `Mailer` mocké : `send_welcome` est appelé quand l'utilisateur devient actif, et **non** appelé quand il reste `waiting`.
- Anti-doublon : pas de renvoi si `welcome_email_sent_at` déjà présent.
- Échec Resend → l'activation réussit quand même.
- `cargo test` + clippy `-D warnings` verts.

## Hors périmètre (reporté)

- Alertes e-mail de programmation des matchs (Phase 3).
- Mail « passe à l'action » pour les commandes (Phase 4).
- File d'envoi / retry (outbox) : reporté.
