-- Tracks which user's cart a resale seat has already been added to — the
-- progressive-fill claim of the Phase 4B bot (Group = Single + Pelouse filter,
-- seats are taken as they appear until the ordered count is reached).
-- NULL = the seat is still on the market.
ALTER TABLE ticket_offers ADD COLUMN claimed_by INTEGER REFERENCES users(id) ON DELETE SET NULL;
