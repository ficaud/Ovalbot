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
