# Procédure de test — capture d'un ajout au panier basket (dump)

Objectif : provoquer une tentative d'ajout au panier sur une revente **basket**
(bien plus fréquente que le rugby), capturer la page reçue + la réponse AJAX du
site, et vérifier que le bot va jusqu'au bout (ou comprendre précisément où ça
bloque).

Le mode basket rattache les rencontres à un id de match **synthétique**
(`BOT_BASKET_MATCH_ID`, ≥ 900000), car le calendrier LNR ne connaît que le
rugby. La demande à servir est donc créée en SQL.

---

## 0. Prérequis

- Identifiants boutique enregistrés pour le compte qui doit acheter
  (user `1` dans les exemples) et `SECRET_KEY` **inchangée** depuis leur
  enregistrement.
- Un canal de notification réel si tu veux être prévenu (`RESEND_ENABLED=true`
  ou Telegram lié). Sans ça, `cart alert processed emailed=true` ne prouve rien
  (les transports no-op renvoient `Ok`).
- Le bot arrêté avant de lancer la session de capture.

---

## 1. Préparer la demande de test

L'interface ne liste que les matchs du calendrier : cette demande se crée en SQL.

```bash
sqlite3 -box ovalbot.sqlite "
INSERT INTO ticket_requests
       (user_id, match_id, kind, quantity, max_price_cents, places, status,
        created_at, updated_at)
VALUES (1, 900001, 'single', 1, 5000, NULL, 'pending',
        strftime('%Y-%m-%dT%H:%M:%fZ','now'),
        strftime('%Y-%m-%dT%H:%M:%fZ','now'))
ON CONFLICT(user_id, match_id) DO UPDATE SET
       kind = excluded.kind,
       quantity = excluded.quantity,
       max_price_cents = excluded.max_price_cents,
       places = excluded.places,
       status = 'pending',
       updated_at = excluded.updated_at;"
```

- `quantity=1` : ne sert que des packs de **1 place**.
- `quantity=2` : accepte aussi un **duo** (ex. pack DENAIN `34681`), ce qui
  valide le POST « pack atomique » sur deux billets.

Vérifier qu'aucune offre n'est déjà réclamée pour cet id (sinon réinitialiser) :

```bash
sqlite3 -box ovalbot.sqlite "
SELECT match_id, pack_id, ticket_id, claimed_by
FROM ticket_offers WHERE match_id = 900001;"
```

---

## 2. Lancer le bot en mode capture

### Docker (recommandé)

**Le dossier de dump est celui du volume monté dans ton compose.** Deux cas :

- compose du dépôt : `./data:/data` → dumps dans `./data/dumps` ;
- compose de déploiement (ex. `data-dev/`) : `./data-dev:/data` → dumps dans
  `./data-dev/dumps`.

Dans tous les cas le chemin **dans le conteneur** est `/data/dumps`. Si tu ne
trouves pas les fichiers, vérifie d'abord quel volume est monté :

```bash
grep -A3 "volumes:" docker-compose.yml
ls -la data/dumps data-dev/dumps 2>/dev/null
```

**Créer le dossier de dump avec le bon propriétaire.** Le conteneur tourne en
`ovalbot` (uid **10001**) et le volume lui appartient : un `mkdir` depuis l'hôte
échoue (`Permission denied`) et le bot logge ensuite
`failed to dump page: No such file or directory`. Crée donc le dossier soit
depuis le conteneur, soit avec `sudo` :

```bash
# a) via le conteneur (aucun sudo, bon propriétaire, chemin valable quel que
#    soit le volume monté)
docker compose run --rm --entrypoint sh ovalbot -c 'mkdir -p /data/dumps'
ls -ld data/dumps data-dev/dumps 2>/dev/null

# b) via sudo sur l'hôte (adapte le chemin du volume)
sudo mkdir -p data-dev/dumps && sudo chown 10001:10001 data-dev/dumps
```

### Configurer le compose (une fois)

Pour ne pas répéter les `-e` à chaque session, déclare les variables dans le
bloc `environment:` du service (elles restent inoffensives en production, avec
les valeurs par défaut) :

```yaml
      BOT_SPORT: ${BOT_SPORT:-rugby}
      BOT_BASKET_MATCH_ID: ${BOT_BASKET_MATCH_ID:-}
      BOT_BASKET_OPPONENT: ${BOT_BASKET_OPPONENT:-}
      CHECKOUT_DUMP_DIR: ${CHECKOUT_DUMP_DIR:-}
      RUST_LOG: ${RUST_LOG:-ovalbot=debug}
```

puis, dans le `.env` de la machine de test :

```
BOT_SPORT=basket
BOT_BASKET_MATCH_ID=900001
BOT_BASKET_OPPONENT=DENAIN
CHECKOUT_DUMP_DIR=/data/dumps
CART_ENABLED=true
CART_DRY_RUN=false
```

Vérification avant la session (le dossier doit être visible **et** inscriptible) :

```bash
docker compose run --rm --entrypoint sh ovalbot -c \
  'echo "DUMP=$CHECKOUT_DUMP_DIR"; ls -la /data; mkdir -p /data/dumps && touch /data/dumps/probe && ls -l /data/dumps/probe'
```

### Lancer la capture

Avec les variables dans `.env` :

```bash
docker compose stop ovalbot
docker compose run --rm --service-ports ovalbot
```

Ou sans toucher au compose, en passant tout en `-e` (les `-e` de `run`
surchargent le compose) :

```bash
docker compose stop ovalbot
docker compose run --rm --service-ports \
  -e RUST_LOG=ovalbot=debug,parser=debug \
  -e BOT_ENABLED=true \
  -e BOT_SPORT=basket \
  -e BOT_BASKET_MATCH_ID=900001 \
  -e BOT_BASKET_OPPONENT=DENAIN \
  -e CART_ENABLED=true \
  -e CART_DRY_RUN=false \
  -e CHECKOUT_DUMP_DIR=/data/dumps \
  ovalbot
```

> Attention : `BOT_SPORT`/`CHECKOUT_DUMP_DIR` doivent figurer **dans la commande
> `run`** (ou dans le compose). S'ils manquent, le bot ne dumpe rien et ne le
> signale pas : `echo $CHECKOUT_DUMP_DIR` vide dans le conteneur est le symptôme.

Quitter la vue des logs sans arrêter le bot : `Ctrl+P` puis `Ctrl+Q`
(`docker logs -f <conteneur>` pour revenir). `Ctrl+C` arrête le bot.

### Depuis les sources

```bash
mkdir -p /tmp/ovalbot-dumps
CHECKOUT_DUMP_DIR=/tmp/ovalbot-dumps \
BOT_ENABLED=true BOT_SPORT=basket BOT_BASKET_MATCH_ID=900001 \
BOT_BASKET_OPPONENT=DENAIN CART_ENABLED=true CART_DRY_RUN=false \
RUST_LOG=ovalbot=debug,parser=debug \
cargo run --bin ovalbot
```

### Re-déclencher une tentative sans attendre de nouvelles places

Le matcher ne tourne que sur des places **fraîches** (`ticket_offers`). Si les
places du moment sont déjà enregistrées (après un `MissingContext`, par
exemple), purge les offres non réclamées du banc d'essai — bot arrêté de
préférence :

```bash
sudo sqlite3 ovalbot.sqlite "
DELETE FROM ticket_offers WHERE match_id = 900001 AND claimed_by IS NULL;"
```

Au tick suivant elles sont ré-enregistrées comme neuves → nouveau plan → nouvelle
tentative, donc un nouveau dump.

---

## 3. Ce qu'on doit voir dans les logs

```
bot scan enabled ... sport="basket" basket_match_id=Some(900001) basket_opponent=Some("denain")
resale link discovered title=DENAIN ... match_id=900001
bot home refresh sport="basket" cards=1 links=1 ...
resale poll: new seat on resale event_id=900001 ... pack_id=... ticket_id=...
purchase matching: checkout plan user_id=1 match_id=900001 kind="single" ...
```

Puis, selon l'issue :

| Log | Signification |
|---|---|
| `checkout: pack added to cart` + `cart alert processed` + `checkout: request closed status="accepted"` | Succès : les places sont dans le panier, va payer (fenêtre ~20 min) |
| `checkout: pack skipped (resale churn)` + `checkout wave added nothing` | Pack disparu entre le scan et le POST, ou POST rejeté — demande laissée `pending` |
| `checkout wave postponed` | Réseau / formulaire expiré / contexte manquant — demande laissée `pending` |
| `add-to-cart context missing from the resale page (bytes=… settings=… ajax_entry=… libraries=… form_build_id=… slider=…)` | Diagnostic : la pièce entre parenthèses à `false` est celle qui manque |
| `checkout login failed` | Login boutique refusé — demande clôturée `failed` (seul cas terminal) |

---

## 4. Récupérer les dumps

Chaque tentative écrit trois fichiers portant le **même id**, dans le dossier
du volume monté (`data/dumps` ou `data-dev/dumps` selon ton compose) :

```
<volume>/dumps/ovalbot_page_<id>.txt      # la page réellement reçue par le bot
<volume>/dumps/ovalbot_response_<id>.txt  # la réponse AJAX (si un POST a eu lieu)
<volume>/dumps/ovalbot_attempt_<id>.txt   # métadonnées
```

Si tu vois dans les logs `failed to dump page: No such file or directory` ou
`failed to dump attempt: …`, le dossier `CHECKOUT_DUMP_DIR` n'existe pas ou
n'est pas inscriptible par l'uid 10001 — reprends l'étape 2 (« Créer le dossier
de dump avec le bon propriétaire »). Aucun dump n'est écrit tant que ce n'est
pas corrigé.

Exemple de métadonnées :

```
attempt=1762016402123456789
pack_id=34681
tickets=["50587", "50590"]
price_min=32.00
price_max=32.00
outcome=added
```

Valeurs d'`outcome` : `added`, `rejected`, `missing_context`, `pack_absent`,
`pack_changed`, `expired_form`, `waiting_queue`, `post_network`.

Lister les tentatives :

```bash
ls -lt data/dumps/ovalbot_attempt_*.txt data-dev/dumps/ovalbot_attempt_*.txt 2>/dev/null | head
grep -l "outcome=added" data/dumps/ovalbot_attempt_*.txt data-dev/dumps/ovalbot_attempt_*.txt 2>/dev/null
```

---

## 5. Analyser

- **`outcome=added`** : comparer `ovalbot_response_<id>.txt` à la réponse de
  succès de référence (`added-to-cart`, `is_already_in_cart`,
  `refreshCartTimeout`) ; garder une copie comme fixture de régression
  (`.html.gz` pour la page, JSON pour la réponse) et ajouter un test.
- **`outcome=rejected`** : la réponse ne contient aucun marqueur de succès. Y
  chercher le marqueur d'échec pour l'ajouter à `pack_added` et affiner la
  sémantique d'erreur.
- **`outcome=missing_context`** : la page (`ovalbot_page_<id>.txt`) n'a ni
  `drupal-settings-json` avec `edit-add-to-cart`, ni `form_build_id` exploitables.
  Vérifier si ces éléments sont présents **échappés** dans les chunks BigPipe
  (`\u0022`), auquel cas étendre l'extraction ; sinon il faudra fabriquer le
  contexte autrement.
- **`outcome=pack_absent` / `pack_changed`** : la page liste des packs mais pas
  ceux du plan — comparer les `data-json` avec le payload capturé dans le
  navigateur (les ids doivent être stables) pour détecter un re-mint.

> ⚠️ Les dumps peuvent contenir des informations de ton compte (page
> authentifiée). Ne les commit pas : analyse-les puis supprime-les.

---

## 6. Nettoyage / retour au rugby

Si les places ne sont pas achetées : retire-les du panier ou laisse-les expirer
(~20 min).

Réinitialiser le banc d'essai :

```bash
sqlite3 -box ovalbot.sqlite "
DELETE FROM ticket_requests WHERE match_id = 900001;
DELETE FROM ticket_offers   WHERE match_id = 900001;
DELETE FROM resale_links    WHERE match_id = 900001;"
```

Puis relancer sans les variables basket (ou `BOT_SPORT=rugby`) :

```bash
docker compose up -d
```

---

## 7. Rappels

- `MissingContext` est désormais **réessayable** : la demande reste `pending`,
  pas besoin de SQL entre deux tentatives. Seul un login refusé clôt en
  `failed` (réarmer via `doc/SQL_HELPERS.md`).
- Ne jamais laisser `CART_DRY_RUN=false` sans surveillance : le bot ajoute de
  vraies places au panier réel.
- Un seul bot à la fois sur la même base (arrêter la prod avant la session de
  capture).
