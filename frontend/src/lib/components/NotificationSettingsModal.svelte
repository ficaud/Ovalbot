<script lang="ts">
  import { invalidateAll } from '$app/navigation';
  import { linkTelegram, unlinkTelegram, updateNotificationSettings } from '#lib/api';

  let {
    telegramLinked,
    notifyEmail,
    notifyTelegram
  }: { telegramLinked: boolean; notifyEmail: boolean; notifyTelegram: boolean } = $props();

  let dialogEl: HTMLDialogElement;

  let linked = $state(false);
  let emailEnabled = $state(true);
  let telegramEnabled = $state(true);
  let telegramPending = $state(false);
  let saving = $state(false);
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
