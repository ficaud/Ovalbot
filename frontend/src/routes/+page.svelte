<script lang="ts">
  import { onMount } from 'svelte';
  import type { PageData } from './$types';
  import type { MatchView, TeamView } from '#lib/types';
  import { getNotifications, getTeamMatches, getTeams, subscribe, unsubscribe } from '#lib/api';

  let { data }: { data: PageData } = $props();

  let teams: TeamView[] = $state([]);
  let selected = $state('stade-rochelais');
  let matches: MatchView[] = $state([]);
  let loading = $state(false);
  let subscribed: number[] = $state([]);
  let pending: number[] = $state([]);

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
    teams = await getTeams();
    subscribed = await getNotifications();
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
      </div>
    {/if}
  {/if}
</section>

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
    cursor: default;
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

  .badge-scheduled {
    background: var(--yellow);
    color: var(--black);
  }

  .badge-tbd {
    background: #f3f4f6;
    color: #374151;
  }
</style>
