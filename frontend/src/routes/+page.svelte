<script lang="ts">
  import { onMount } from 'svelte';
  import { invalidateAll } from '$app/navigation';
  import type { PageData } from './$types';
  import type { MatchView, TeamView } from '#lib/types';
  import {
    getNotifications,
    getTeamMatches,
    getTeams,
    linkTelegram,
    subscribe,
    unsubscribe,
    unlinkTelegram,
    updateNotificationSettings
  } from '#lib/api';

  let { data }: { data: PageData } = $props();

  let teams: TeamView[] = $state([]);
  let selected = $state('stade-rochelais');
  let matches: MatchView[] = $state([]);
  let loading = $state(false);
  let subscribed: number[] = $state([]);
  let pending: number[] = $state([]);

  let dialogEl: HTMLDialogElement;
  let telegramLinked = $state(false);
  let emailEnabled = $state(true);
  let telegramEnabled = $state(true);
  let telegramPending = $state(false);
  let saving = $state(false);
  let error = $state('');
  let notice = $state('');

  function openSettings() {
    error = '';
    notice = '';
    dialogEl?.showModal();
  }

  function closeSettings() {
    dialogEl?.close();
  }

  async function connectTelegram() {
    telegramPending = true;
    error = '';
    notice = '';
    try {
      const { url } = await linkTelegram();
      window.open(url, '_blank', 'noopener');
      notice = 'Open Telegram and press Start to finish connecting.';
    } catch (err) {
      error = err instanceof Error ? err.message : 'Telegram is unavailable';
    } finally {
      telegramPending = false;
    }
  }

  async function disconnectTelegram() {
    telegramPending = true;
    error = '';
    notice = '';
    try {
      await unlinkTelegram();
      telegramLinked = false;
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to disconnect Telegram';
    } finally {
      telegramPending = false;
    }
  }

  async function saveSettings() {
    saving = true;
    error = '';
    notice = '';
    try {
      await updateNotificationSettings(emailEnabled, telegramEnabled);
      await invalidateAll();
      telegramLinked = data.user.telegram_linked;
      notice = 'Settings saved.';
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to save settings';
    } finally {
      saving = false;
    }
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
    telegramLinked = data.user.telegram_linked;
    emailEnabled = data.user.notify_email;
    telegramEnabled = data.user.notify_telegram;
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
        <p class="muted">Manage bot</p>
        <a href="/" class="btn btn-secondary">Bot settings</a>
      </div>
    {/if}

    <div class="card">
      <h3>Notifications</h3>
      <p class="muted">Choose how you want to be alerted when a match gets a date.</p>
      <button class="btn btn-secondary" type="button" onclick={openSettings}>
        Notification settings
      </button>
    </div>
  {/if}
</section>

<dialog bind:this={dialogEl} class="modal">
  <div class="modal__head">
    <h2>Notification settings</h2>
    <button class="modal__close" type="button" aria-label="Close" onclick={closeSettings}>
      &times;
    </button>
  </div>

  {#if error}
    <div class="alert alert-error">{error}</div>
  {/if}
  {#if notice}
    <div class="alert alert-info">{notice}</div>
  {/if}

  <div class="field">
    <h3>Telegram</h3>
    {#if telegramLinked}
      <p class="muted">Your Telegram account is connected.</p>
      <button
        class="btn btn-secondary btn-sm"
        type="button"
        onclick={disconnectTelegram}
        disabled={telegramPending}
      >
        Disconnect Telegram
      </button>
    {:else}
      <p class="muted">Connect Telegram to get match alerts there too.</p>
      <button
        class="btn btn-secondary btn-sm"
        type="button"
        onclick={connectTelegram}
        disabled={telegramPending}
      >
        Connect Telegram
      </button>
    {/if}
  </div>

  <div class="field">
    <h3>Channels</h3>
    <label class="switch">
      <input type="checkbox" bind:checked={emailEnabled} />
      <span>Email alerts</span>
    </label>
    <label class="switch" class:switch--disabled={!telegramLinked}>
      <input type="checkbox" bind:checked={telegramEnabled} disabled={!telegramLinked} />
      <span>Telegram alerts</span>
    </label>
    {#if !telegramLinked}
      <p class="hint">Connect Telegram first to enable Telegram alerts.</p>
    {/if}
  </div>

  <div class="modal__actions">
    <button class="btn btn-secondary" type="button" onclick={closeSettings}>Close</button>
    <button class="btn btn-primary" type="button" onclick={saveSettings} disabled={saving}>
      {saving ? 'Saving…' : 'Save'}
    </button>
  </div>
</dialog>

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

  .modal {
    width: min(440px, calc(100vw - 32px));
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 20px 24px;
    background: var(--surface);
    color: var(--ink);
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.2);
  }

  .modal::backdrop {
    background: rgba(0, 0, 0, 0.5);
  }

  .modal__head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  .modal__head h2 {
    margin: 0;
  }

  .modal__close {
    border: none;
    background: transparent;
    font-size: 1.5rem;
    line-height: 1;
    color: var(--muted);
    cursor: pointer;
    padding: 0 4px;
  }

  .modal__actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 20px;
  }

  .switch {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 8px 0;
    cursor: pointer;
  }

  .switch--disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
