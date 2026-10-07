export type Role = 'admin' | 'user';
export type Status = 'active' | 'waiting';

export interface UserView {
  id: number;
  email: string;
  status: Status;
  role: Role;
  created_at: string;
  activated_at: string | null;
}

export interface VersionInfo {
  version: string;
}

export interface RegisterResult {
  email: string;
  status: Status;
}
