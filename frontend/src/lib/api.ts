import type {
  MatchView,
  RegisterResult,
  TeamView,
  TicketRequestDraft,
  TicketRequestInput,
  TicketRequestView,
  UserView,
  VersionInfo
} from './types';

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

export async function getTeams(): Promise<TeamView[]> {
  return parse<TeamView[]>(await fetch('/api/teams'));
}

export async function getTeamMatches(teamId: string): Promise<MatchView[]> {
  return parse<MatchView[]>(await fetch(`/api/teams/${teamId}/matches`));
}

export async function getNotifications(): Promise<number[]> {
  return parse<number[]>(await fetch('/api/notifications'));
}

export async function subscribe(matchId: number): Promise<void> {
  const res = await fetch(`/api/notifications/${matchId}`, { method: 'POST' });
  if (!res.ok) throw new ApiError(res.status, 'Subscription failed');
}

export async function unsubscribe(matchId: number): Promise<void> {
  const res = await fetch(`/api/notifications/${matchId}`, { method: 'DELETE' });
  if (!res.ok) throw new ApiError(res.status, 'Unsubscription failed');
}

export async function linkTelegram(): Promise<{ url: string }> {
  return parse<{ url: string }>(await fetch('/api/telegram/link', { method: 'POST' }));
}

export async function unlinkTelegram(): Promise<void> {
  const res = await fetch('/api/telegram/link', { method: 'DELETE' });
  if (!res.ok) throw new ApiError(res.status, 'Failed to disconnect Telegram');
}

export async function updateNotificationSettings(
  emailEnabled: boolean,
  telegramEnabled: boolean
): Promise<void> {
  const res = await fetch('/api/notifications/settings', {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ email_enabled: emailEnabled, telegram_enabled: telegramEnabled })
  });
  if (!res.ok) throw new ApiError(res.status, 'Failed to save notification settings');
}

function toTicketRequestInput(draft: TicketRequestDraft): TicketRequestInput {
  switch (draft.kind) {
    case 'single':
      return {
        kind: 'single',
        quantity: draft.single.quantity,
        max_price_cents: draft.single.maxPriceCents!
      };
    case 'duo':
      return { kind: 'duo', max_price_cents: draft.duo.maxPriceCents! };
    case 'group':
      return { kind: 'group', places: draft.group.places };
    case 'notify':
      return { kind: 'notify' };
  }
}

export async function listRequests(): Promise<TicketRequestView[]> {
  return parse<TicketRequestView[]>(await fetch('/api/requests'));
}

export function toTicketRequestDraft(view: TicketRequestView): TicketRequestDraft {
  return {
    matchId: view.match_id,
    kind: view.kind,
    single: { quantity: view.quantity ?? 1, maxPriceCents: view.max_price_cents },
    duo: { maxPriceCents: view.max_price_cents },
    group: { places: view.places ?? 4 }
  };
}

export async function putRequest(draft: TicketRequestDraft): Promise<void> {
  const res = await fetch(`/api/requests/${draft.matchId}`, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(toTicketRequestInput(draft))
  });
  if (!res.ok) throw new ApiError(res.status, 'Failed to send ticket request');
}

export async function deleteRequest(matchId: number): Promise<void> {
  const res = await fetch(`/api/requests/${matchId}`, { method: 'DELETE' });
  if (!res.ok) throw new ApiError(res.status, 'Failed to remove ticket request');
}

/**
 * Saves (or replaces) the user's billetterie credentials behind the bot's
 * automatic add-to-cart. The server encrypts the password at rest and never
 * returns it.
 */
export async function setShopCredentials(email: string, password: string): Promise<void> {
  const res = await fetch('/api/shop/credentials', {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ email, password })
  });
  if (!res.ok) throw new ApiError(res.status, 'Failed to save shop credentials');
}

/** Clears the user's stored billetterie credentials (idempotent). */
export async function deleteShopCredentials(): Promise<void> {
  const res = await fetch('/api/shop/credentials', { method: 'DELETE' });
  if (!res.ok) throw new ApiError(res.status, 'Failed to clear shop credentials');
}
