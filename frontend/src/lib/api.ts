import type { RegisterResult, UserView, VersionInfo } from './types';

export class ApiError extends Error {
  status: number;
  code?: string;

  constructor(status: number, message: string, code?: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
  }
}

async function parse<T>(res: Response): Promise<T> {
  if (res.ok) return (await res.json()) as T;

  let message = res.statusText || 'Request failed';
  let code: string | undefined;
  try {
    const body = (await res.json()) as { message?: string; error?: string };
    message = body.message ?? message;
    code = body.error;
  } catch {
    // non-JSON error body, keep the status text
  }

  throw new ApiError(res.status, message, code);
}

function post<T>(body: T): RequestInit {
  return {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body)
  };
}

export async function getMe(): Promise<UserView | null> {
  const res = await fetch('/api/me');
  if (res.status === 401) return null;
  return parse<UserView>(res);
}

export async function getVersion(): Promise<VersionInfo> {
  return parse<VersionInfo>(await fetch('/api/version'));
}

export async function login(email: string, password: string): Promise<UserView> {
  return parse<UserView>(await fetch('/api/login', post({ email, password })));
}

export async function register(email: string, password: string): Promise<RegisterResult> {
  return parse<RegisterResult>(await fetch('/api/register', post({ email, password })));
}

export async function logout(): Promise<void> {
  const res = await fetch('/api/logout', { method: 'POST' });
  if (!res.ok) throw new ApiError(res.status, 'Logout failed');
}

export async function listUsers(): Promise<UserView[]> {
  return parse<UserView[]>(await fetch('/api/admin/users'));
}

export async function activateUser(id: number): Promise<void> {
  const res = await fetch(`/api/admin/users/${id}/activate`, { method: 'POST' });
  if (!res.ok) throw new ApiError(res.status, 'Activation failed');
}

export async function deleteUser(id: number): Promise<void> {
  const res = await fetch(`/api/admin/users/${id}`, { method: 'DELETE' });
  if (!res.ok) throw new ApiError(res.status, 'Deletion failed');
}
