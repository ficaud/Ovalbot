# SQL Helpers requests 

_Note : These commands can also be run on .sqlite files created by the ovalbot on a raspberry pi for instance.

How to get the list of ticket requests emit by users:

```sql
sqlite3 -box ovalbot.sqlite "
SELECT u.email,
       tr.match_id,
       tr.kind,
       tr.quantity,
       tr.max_price_cents,
       tr.places,
       tr.status,
       tr.updated_at
FROM ticket_requests tr
JOIN users u ON u.id = tr.user_id
ORDER BY u.email, tr.match_id;"
```

Detailed view : un match_id par ligne

```sql
sqlite3 -box ovalbot.sqlite "
SELECT u.email,
       mn.match_id,
       mn.created_at AS notified_at
FROM match_notifications mn
JOIN users u ON u.id = mn.user_id
ORDER BY u.email, mn.match_id;"
```

Per user : one line with all his match_ids

```sql
sqlite3 -box ovalbot.sqlite "
SELECT u.email,
       COUNT(*)              AS nb_alerts,
       GROUP_CONCAT(mn.match_id) AS match_ids
FROM match_notifications mn
JOIN users u ON u.id = mn.user_id
GROUP BY u.id
ORDER BY nb_alerts DESC, u.email;"
```

List out users that have registered their telegram account

```sql
sqlite3 -box ovalbot.sqlite "
SELECT u.id,
       u.email,
       u.telegram_id,
       u.notify_email,
       u.notify_telegram
FROM users u
WHERE u.telegram_id IS NOT NULL
ORDER BY u.email;"
```

List out all the tickets requests that have been sent to users

```sql
sqlite3 -box ovalbot.sqlite "
SELECT u.email,
       tr.match_id,
       tr.kind,
       tr.quantity,
       tr.max_price_cents,
       tr.places,
       tr.status,
       tr.created_at,
       tr.updated_at
FROM ticket_requests tr
JOIN users u ON u.id = tr.user_id
ORDER BY u.email, tr.match_id;"
```

Inspect the bot's resale links (`resale_links`: what home-page discovery
learned, what the direct poll did):

```sql
sqlite3 -box ovalbot.sqlite "
SELECT title,
       date,
       match_id,
       resale_link,
       first_seen_at,
       last_seen_at,
       last_scanned_at,
       last_scan_status
FROM resale_links
ORDER BY last_seen_at DESC;"
```

Quick health check of the same table (one line per poll outcome):

```sql
sqlite3 -box ovalbot.sqlite "
SELECT last_scan_status,
       COUNT(*) AS links
FROM resale_links
GROUP BY last_scan_status;"
```

List all the tickets detected on resale (`ticket_offers`: one row per
seat the bot ever recorded, ordered by detection time):

```sql
sqlite3 -box ovalbot.sqlite "
SELECT match_id,
       title,
       date,
       pack_id,
       ticket_id,
       first_seen_at
FROM ticket_offers
ORDER BY first_seen_at DESC;"
```

Same offers grouped per match (one line per match, all recorded seats):

```sql
sqlite3 -box ovalbot.sqlite "
SELECT match_id,
       title,
       date,
       COUNT(*)           AS seats,
       MIN(first_seen_at) AS first_seen
FROM ticket_offers
GROUP BY match_id, title, date
ORDER BY first_seen DESC;"
```

## Re-arm a ticket request closed as `failed`

The bot closes a request as `failed` on a terminal checkout error (refused shop
login or a layout change on the resale page), and **never retries it
automatically**. The frontend then locks the request: it no longer counts as
`pending`, so the modal shows “This request is now ‘FAILED’ and can no longer be
modified”. Re-arming is a deliberate SQL action:

1. List the failed requests (and check which match/user they belong to):

```sql
sqlite3 -box ovalbot.sqlite "
SELECT u.email,
       tr.match_id,
       tr.kind,
       tr.quantity,
       tr.places,
       tr.status,
       tr.updated_at
FROM ticket_requests tr
JOIN users u ON u.id = tr.user_id
WHERE tr.status = 'failed'
ORDER BY tr.updated_at DESC;"
```

2. Put one request back into play (`AND status = 'failed'` keeps the command
   idempotent: it does nothing if the request already moved on):

```sql
sqlite3 -box ovalbot.sqlite "
UPDATE ticket_requests
   SET status = 'pending',
       updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
 WHERE user_id = 2
   AND match_id = 11877
   AND status = 'failed';"
```

3. Check the result (the modal is editable again once the request is
   `pending`):

```sql
sqlite3 -box ovalbot.sqlite "
SELECT tr.user_id,
       u.email,
       tr.match_id,
       tr.kind,
       tr.status,
       tr.updated_at
FROM ticket_requests tr
JOIN users u ON u.id = tr.user_id
WHERE tr.match_id = 11877;"
```

A `pending` request is only served by the **next new-seats wave**: the bot
matches freshly listed resale seats, never the ones it already recorded (or the
ones already sitting in a cart). To re-trigger a wave on demand for a test,
forget the unclaimed offers of the match — they will be re-recorded as new on
the next poll:

```sql
sqlite3 -box ovalbot.sqlite "
DELETE FROM ticket_offers
 WHERE match_id = 11877
   AND claimed_by IS NULL;"
```

> Warning: those rows are re-announced as fresh seats, so a `notify` subscriber
> on the same match receives the seats alert again — that is expected for a
> manual test, not something to run routinely.
