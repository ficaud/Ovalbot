<script lang="ts">
  import { invalidateAll } from '$app/navigation';
  import { deleteShopCredentials, setShopCredentials } from '#lib/api';

  let {
    shopEmail
  }: { shopEmail: string | null } = $props();

  let dialogEl: HTMLDialogElement;

  let email = $state('');
  let password = $state('');
  let saving = $state(false);
  let error = $state('');
  let notice = $state('');

  export function open() {
    // Only the shop email is readable back; the password field always starts
    // empty (a filled one would be a fake sense of "known password").
    email = shopEmail ?? '';
    password = '';
    error = '';
    notice = '';
    dialogEl?.showModal();
  }

  function close() {
    dialogEl?.close();
  }

  const emailValid = $derived(email.trim().includes('@'));
  const canSave = $derived(
    emailValid && password.length > 0 && password.length <= 1024 && !saving
  );

  async function save() {
    saving = true;
    error = '';
    notice = '';
    try {
      await setShopCredentials(email.trim(), password);
      await invalidateAll();
      password = '';
      notice = 'Billetterie account saved. The bot can now add seats to your cart.';
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to save credentials';
    } finally {
      saving = false;
    }
  }

  async function disconnect() {
    saving = true;
    error = '';
    notice = '';
    try {
      await deleteShopCredentials();
      await invalidateAll();
      email = '';
      notice = 'Billetterie account removed.';
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to remove credentials';
    } finally {
      saving = false;
    }
  }
</script>

<dialog bind:this={dialogEl} class="modal">
  <div class="modal__head">
    <h2>Billetterie account</h2>
    <button class="modal__close" type="button" aria-label="Close" onclick={close}>&times;</button>
  </div>

  {#if error}
    <div class="alert alert-error">{error}</div>
  {/if}
  {#if notice}
    <div class="alert alert-info">{notice}</div>
  {/if}

  <div class="field">
    <p class="muted">
      Store your club's ticketing ({'billetterie.staderochelais.com'}) credentials so the bot can
      automatically add matching resale seats to <strong>your own</strong> cart, then alert you to
      pay within the cart's lifetime (~20 minutes).
    </p>
    <p class="hint">
      The password is stored encrypted on the server and never displayed again. Entering it here
      replaces the previous one.
    </p>
  </div>

  <div class="field">
    <label class="shop-label" for="shop-email">Billetterie email</label>
    <input
      id="shop-email"
      type="email"
      bind:value={email}
      placeholder="you@billetterie-account"
      autocomplete="off"
    />
    <label class="shop-label" for="shop-password">Billetterie password</label>
    <input
      id="shop-password"
      type="password"
      bind:value={password}
      placeholder={shopEmail ? '•••••••• (already stored — retype to change)' : '••••••••'}
      autocomplete="new-password"
    />
  </div>

  <div class="modal__actions">
    {#if shopEmail}
      <button
        class="btn btn-secondary btn-danger-ghost"
        type="button"
        onclick={disconnect}
        disabled={saving}
      >
        Remove credentials
      </button>
    {/if}
    <button class="btn btn-primary" type="button" onclick={save} disabled={!canSave}>
      {saving ? 'Saving…' : 'Save'}
    </button>
  </div>
</dialog>

<style>
  .shop-label {
    display: block;
    margin: 8px 0 4px;
    font-weight: 600;
    font-size: 0.9rem;
  }

  .btn-danger-ghost:not(:hover) {
    color: var(--danger, #c0392b);
  }
</style>
