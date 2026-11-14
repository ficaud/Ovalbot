export type Role = 'admin' | 'user';
export type Status = 'active' | 'waiting';

export interface UserView {
  id: number;
  email: string;
  status: Status;
  role: Role;
  created_at: string;
  activated_at: string | null;
  telegram_linked: boolean;
  notify_email: boolean;
  notify_telegram: boolean;
  /** The saved billetterie email for the bot's automatic add-to-cart (null = not configured). */
  shop_email: string | null;
}

export interface VersionInfo {
  version: string;
}

export interface RegisterResult {
  email: string;
  status: Status;
}

export interface TeamView {
  id: string;
  name: string;
}

/** Bot resale statistics for one match (null = never seen on resale by the bot). */
export interface MatchStats {
  /** Distinct seats the bot has ever seen on this match's resale. */
  seats_seen: number;
}

export interface MatchView {
  id: number;
  team: string;
  team_id: string;
  opponent: string;
  opponent_id: string;
  kickoff: string;
  status: string;
  stats: MatchStats | null;
}

export type TicketKind = 'single' | 'duo' | 'group' | 'notify';

export interface TicketRequestDraft {
  matchId: number;
  kind: TicketKind;
  single: { quantity: number; maxPriceCents: number | null };
  duo: { maxPriceCents: number | null };
  group: { places: number };
}

export type TicketRequestInput =
  | { kind: 'single'; quantity: number; max_price_cents: number }
  | { kind: 'duo'; max_price_cents: number }
  | { kind: 'group'; places: number }
  | { kind: 'notify' };

export type TicketRequestStatus =
  | 'pending'
  | 'accepted'
  | 'purchased'
  | 'failed'
  | 'expired'
  | 'cancelled';

export interface TicketRequestView {
  match_id: number;
  kind: TicketKind;
  quantity: number | null;
  max_price_cents: number | null;
  places: number | null;
  status: TicketRequestStatus;
  created_at: string;
  updated_at: string;
}
