<script lang="ts">
  import { invalidateAll } from '$app/navigation';
  import {
    getTelegramChannel,
    linkTelegram,
    setTelegramChannel,
    unlinkTelegram,
    updateNotificationSettings
  } from '#lib/api';

  let {
    telegramLinked,
    notifyEmail,
    notifyTelegram,
    isAdmin
  }: {
    telegramLinked: boolean;
    notifyEmail: boolean;
    notifyTelegram: boolean;
    isAdmin: boolean;
  } = $props();

  let dialogEl: HTMLDialogElement;

  let linked = $state(false);
  let emailEnabled = $state(true);
  let telegramEnabled = $state(true);
  let telegramPending = $state(false);
  let saving = $state(false);
  let channelId = $state('');
  let botToken = $state('');
  let hasToken = $state(false);
  let channelSaving = $state(false);
  let error = $state('');
  let notice = $state('');

  $effect(() => {
    linked = telegramLinked;
  });

  export function open() {
    linked = telegramLinked;
    emailEnabled = notifyEmail;
    telegramEnabled = notifyTelegram;
    error = '';
    notice = '';
    dialogEl?.showModal();
    if (isAdmin) {
      void getTelegramChannel()
        .then((channel) => {
          channelId = channel.chat_id ?? '';
          hasToken = channel.has_token;
          botToken = '';
        })
        .catch(() => {
          channelId = '';
          hasToken = false;
          botToken = '';
        });
    }
  }

  function close() {
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
      linked = false;
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to disconnect Telegram';
    } finally {
      telegramPending = false;
    }
  }

  async function saveChannel() {
    channelSaving = true;
    error = '';
    notice = '';
    try {
      const channel = await setTelegramChannel(channelId.trim(), botToken.trim() || null);
      channelId = channel.chat_id ?? '';
      hasToken = channel.has_token;
      botToken = '';
      notice = 'Broadcast channel saved.';
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to save the broadcast channel';
    } finally {
      channelSaving = false;
    }
  }

  async function removeChannel() {
    channelSaving = true;
    error = '';
    notice = '';
    try {
      await setTelegramChannel(null);
      channelId = '';
      hasToken = false;
      botToken = '';
      notice = 'Broadcast channel removed.';
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to remove the broadcast channel';
    } finally {
      channelSaving = false;
    }
  }

  async function save() {
    saving = true;
    error = '';
    notice = '';
    try {
      await updateNotificationSettings(emailEnabled, telegramEnabled);
      await invalidateAll();
      notice = 'Settings saved.';
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to save settings';
    } finally {
      saving = false;
    }
  }
</script>

<dialog bind:this={dialogEl} class="modal">
  <div class="modal__head">
    <h2>Notification settings</h2>
    <button class="modal__close" type="button" aria-label="Close" onclick={close}>&times;</button>
  </div>

  {#if error}
    <div class="alert alert-error">{error}</div>
  {/if}
  {#if notice}
    <div class="alert alert-info">{notice}</div>
  {/if}

  <div class="field">
    <h3>Telegram</h3>
    {#if linked}
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

  {#if isAdmin}
    <div class="field">
      <h3>Broadcast channel (admin)</h3>
      <p class="muted">
        Mirror the alerts you are subscribed to (resale seats, schedule, cart) into a Telegram
        channel, using a dedicated bot of your own. This is in addition to your own
        notifications.
      </p>
      <p class="hint">
        Create a bot with @BotFather, add it to the channel as admin, then paste its token and the
        channel id below.
      </p>
      <label class="channel-label" for="broadcast-channel">Channel chat id</label>
      <input
        id="broadcast-channel"
        type="text"
        bind:value={channelId}
        placeholder="-1001234567890 or @channel"
        autocomplete="off"
      />
      <label class="channel-label" for="broadcast-token">Bot token</label>
      <input
        id="broadcast-token"
        type="password"
        bind:value={botToken}
        placeholder={hasToken ? '•••••••• (already stored — retype to change)' : '123456789:AA…'}
        autocomplete="off"
      />
      <p class="hint">Chat id: numeric (-100…) or public @username. The token is never shown again.</p>
      <div class="modal__actions">
        {#if channelId}
          <button
            class="btn btn-secondary btn-danger-ghost"
            type="button"
            onclick={removeChannel}
            disabled={channelSaving}
          >
            Remove channel
          </button>
        {/if}
        <button
          class="btn btn-primary"
          type="button"
          onclick={saveChannel}
          disabled={channelSaving || !channelId.trim() || (!botToken.trim() && !hasToken)}
        >
          {channelSaving ? 'Saving…' : 'Save channel'}
        </button>
      </div>
    </div>
  {/if}

  <div class="field">
    <h3>Channels</h3>
    <label class="switch">
      <input type="checkbox" bind:checked={emailEnabled} />
      <span>Email alerts</span>
    </label>
    <label class="switch" class:switch--disabled={!linked}>
      <input type="checkbox" bind:checked={telegramEnabled} disabled={!linked} />
      <span>Telegram alerts</span>
    </label>
    {#if !linked}
      <p class="hint">Connect Telegram first to enable Telegram alerts.</p>
    {/if}
  </div>

  <div class="modal__actions">
    <button class="btn btn-secondary" type="button" onclick={close}>Close</button>
    <button class="btn btn-primary" type="button" onclick={save} disabled={saving}>
      {saving ? 'Saving…' : 'Save'}
    </button>
  </div>
</dialog>

<style>
  .channel-label {
    display: block;
    margin: 8px 0 4px;
    font-weight: 600;
    font-size: 0.9rem;
  }

  .btn-danger-ghost:not(:hover) {
    color: var(--danger, #c0392b);
  }
</style>
