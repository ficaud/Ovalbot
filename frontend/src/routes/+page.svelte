<script lang="ts">
  import { onMount } from 'svelte';
  import type { PageData } from './$types';
  import type {
    MatchView,
    TeamView,
    TicketRequestDraft,
    TicketRequestStatus
  } from '#lib/types';
  import {
    deleteRequest,
    getNotifications,
    getTeamMatches,
    getTeams,
    listRequests,
    putRequest,
    subscribe,
    toTicketRequestDraft,
    unsubscribe
  } from '#lib/api';
  import NotificationSettingsModal from '#lib/components/NotificationSettingsModal.svelte';
  import TicketRequestModal from '#lib/components/TicketRequestModal.svelte';

  let { data }: { data: PageData } = $props();

  let teams: TeamView[] = $state([]);
  let selected = $state('stade-rochelais');
  let matches: MatchView[] = $state([]);
  let loading = $state(false);
  let subscribed: number[] = $state([]);
  let pending: number[] = $state([]);
  let requests = $state<Record<number, TicketRequestDraft>>({});
  let statuses = $state<Record<number, TicketRequestStatus>>({});

  let settingsModal: ReturnType<typeof NotificationSettingsModal> | undefined;
  let requestModal: ReturnType<typeof TicketRequestModal> | undefined;

  // The tickets bot currently only scrapes the Stade Rochelais billetterie
  // (the club's upcoming home games); requests for any other team cannot be
  // serviced. Extend when a new club parser is added.
  const BOT_SUPPORTED_TEAM_IDS = ['stade-rochelais'];

  const BOT_UNAVAILABLE_TITLE =
    'The bot only covers Stade Rochelais home games for now.';

  function isBotSupported(match: MatchView): boolean {
    return BOT_SUPPORTED_TEAM_IDS.includes(match.team_id);
  }

  function isRequestable(match: MatchView): boolean {
    // Existing requests stay reachable so they can still be removed.
    return (
      isBotSupported(match) ||
      requests[match.id] !== undefined ||
      statuses[match.id] !== undefined
    );
  }

  async function saveRequest(draft: TicketRequestDraft) {
    await putRequest(draft);
    requests = { ...requests, [draft.matchId]: draft };
    statuses = { ...statuses, [draft.matchId]: 'pending' };
  }

  async function removeRequest(matchId: number) {
    await deleteRequest(matchId);
    const next = { ...requests };
    delete next[matchId];
    requests = next;
    const nextStatuses = { ...statuses };
    delete nextStatuses[matchId];
    statuses = nextStatuses;
  }

  async function refreshMatches() {
    loading = true;
    try {
      matches = await getTeamMatches(selected);
    } finally {
      loading = false;
    }
  }

  async function toggleNotification(match: MatchView) {
    if (pending.includes(match.id)) return;
    pending = [...pending, match.id];
    try {
      if (subscribed.includes(match.id)) {
        await unsubscribe(match.id);
        subscribed = subscribed.filter((id) => id !== match.id);
      } else {
        await subscribe(match.id);
        subscribed = [...subscribed, match.id];
      }
    } finally {
      pending = pending.filter((id) => id !== match.id);
    }
  }

  onMount(async () => {
    const [teamList, notifications, stored] = await Promise.all([
      getTeams(),
      getNotifications(),
      // Non-critical: the page stays usable if the session just expired.
      listRequests().catch(() => [])
    ]);
    teams = teamList;
    subscribed = notifications;
    requests = Object.fromEntries(
      stored.map((view) => [view.match_id, toTicketRequestDraft(view)])
    );
    statuses = Object.fromEntries(stored.map((view) => [view.match_id, view.status]));
    await refreshMatches();
  });
</script>

<section class="page">
  {#if data.user.status === 'waiting'}
    <div class="card">
      <h2>You're on the waitlist</h2>
      <p class="muted">
        OvalBot is limited to 10 active members. Your account is confirmed — a spot will open up
        soon and you'll be able to browse the match calendar.
      </p>
    </div>
  {:else}
    <div class="card">
      <h2>Home matches</h2>
      <p class="muted">Pick a team to see its upcoming home games.</p>
      <label class="field" for="team-select">Team</label>
      <select
        id="team-select"
        class="input"
        bind:value={selected}
        onchange={refreshMatches}
      >
        {#each teams as team (team.id)}
          <option value={team.id}>{team.name}</option>
        {/each}
      </select>
    </div>

    {#if loading}
      <p class="muted">Loading…</p>
    {:else if matches.length === 0}
      <div class="card">
        <p class="muted">No upcoming home matches for this team.</p>
      </div>
    {:else}
      <div class="matches">
        {#each matches as match (match.id)}
          <div class="match-card">
            <div class="match-card__teams">
              <span class="match-card__club">
                <img
                  class="match-card__logo"
                  src={`/teams/${match.team_id}.png`}
                  alt={match.team}
                />
                <span class="match-card__name">{match.team}</span>
              </span>
              <span class="match-card__vs">vs</span>
              <span class="match-card__club match-card__club--away">
                <span class="match-card__name">{match.opponent}</span>
                <img
                  class="match-card__logo"
                  src={`/teams/${match.opponent_id}.png`}
                  alt={match.opponent}
                />
              </span>
            </div>
            <div class="match-card__footer">
              <span class="match-card__kickoff">{match.kickoff}</span>
              <div class="match-card__meta">
                <button
                  class="bell"
                  class:bell--active={requests[match.id] !== undefined}
                  type="button"
                  disabled={!isRequestable(match)}
                  aria-label="Ticket request"
                  title={isRequestable(match)
                    ? 'Ticket request'
                    : BOT_UNAVAILABLE_TITLE}
                  onclick={() =>
                    requestModal?.open(match, requests[match.id] ?? null, statuses[match.id])}
                >
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                  >
                    <path d="M12 8V4H8" />
                    <rect width="16" height="12" x="4" y="8" rx="2" />
                    <path d="M2 14h2" />
                    <path d="M20 14h2" />
                    <path d="M15 13v2" />
                    <path d="M9 13v2" />
                  </svg>
                </button>
                {#if match.status === 'not scheduled'}
                  <button
                    class="bell"
                    class:bell--active={subscribed.includes(match.id)}
                    type="button"
                    disabled={pending.includes(match.id)}
                    aria-pressed={subscribed.includes(match.id)}
                    aria-label={subscribed.includes(match.id)
                      ? 'Turn off notifications for this match'
                      : 'Be notified when this match gets a date'}
                    title={subscribed.includes(match.id)
                      ? 'Turn off notifications for this match'
                      : 'Be notified when this match gets a date'}
                    onclick={() => toggleNotification(match)}
                  >
                    <svg
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="2"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      aria-hidden="true"
                    >
                      <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
                      <path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
                    </svg>
                  </button>
                {/if}
                <span
                  class="badge {match.status === 'scheduled'
                    ? 'badge-scheduled'
                    : 'badge-tbd'}"
                >
                  {match.status}
                </span>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    {#if data.user.role === 'admin'}
      <div class="card">
        <h3>Administration</h3>
        <p class="muted">Manage accounts and the waitlist.</p>
        <a href="/admin" class="btn btn-secondary">Manage accounts</a>
        <p class="muted">Manage bot</p>
        <a href="/" class="btn btn-secondary">Bot settings</a>
      </div>
    {/if}

    <div class="card">
      <h3>Notifications</h3>
      <p class="muted">Choose how you want to be alerted when a match gets a date.</p>
      <button class="btn btn-secondary" type="button" onclick={() => settingsModal?.open()}>
        Notification settings
      </button>
    </div>
  {/if}
</section>

<NotificationSettingsModal
  bind:this={settingsModal}
  telegramLinked={data.user.telegram_linked}
  notifyEmail={data.user.notify_email}
  notifyTelegram={data.user.notify_telegram}
/>

<TicketRequestModal
  bind:this={requestModal}
  onsave={saveRequest}
  onremove={removeRequest}
/>

<style>
  .matches {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .match-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    text-align: left;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px 20px;
    font-family: inherit;
    font-size: 1rem;
    color: var(--ink);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.06);
    transition: border-color 0.15s ease;
  }

  .bell {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border-radius: 50%;
    border: 1px solid var(--border);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    transition:
      color 0.15s ease,
      border-color 0.15s ease,
      background 0.15s ease;
  }

  .bell svg {
    width: 16px;
    height: 16px;
  }

  .bell:hover:not(:disabled) {
    border-color: var(--yellow);
    color: var(--black);
  }

  .bell:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .bell--active {
    background: var(--yellow);
    border-color: var(--yellow);
    color: var(--black);
  }

  .match-card:hover {
    border-color: var(--yellow);
  }

  .match-card__teams {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .match-card__club {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 1;
    min-width: 0;
  }

  .match-card__club--away {
    justify-content: flex-end;
  }

  .match-card__logo {
    width: 28px;
    height: 28px;
    object-fit: contain;
    flex-shrink: 0;
  }

  .match-card__name {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .match-card__vs {
    color: var(--muted);
    font-weight: 500;
    flex-shrink: 0;
  }

  .match-card__footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .match-card__kickoff {
    color: var(--muted);
    font-size: 0.9rem;
  }

  .match-card__meta {
    display: flex;
    align-items: center;
    gap: 10px;
  }
</style>
