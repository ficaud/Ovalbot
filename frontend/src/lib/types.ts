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

export interface MatchView {
  id: number;
  team: string;
  team_id: string;
  opponent: string;
  opponent_id: string;
  kickoff: string;
  status: string;
}
