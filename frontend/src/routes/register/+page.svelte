<script lang="ts">
  import { goto } from '$app/navigation';
  import { register } from '#lib/api';

  let email = $state('');
  let password = $state('');
  let error = $state('');
  let loading = $state(false);

  async function onSubmit(e: SubmitEvent) {
    e.preventDefault();
    error = '';
    loading = true;
    try {
      const result = await register(email, password);
      const notice = result.status === 'active' ? 'active' : 'waiting';
      await goto(`/login?registered=${notice}`);
    } catch (err) {
      error = err instanceof Error ? err.message : 'Something went wrong';
      loading = false;
    }
  }
</script>

<div class="auth-shell">
  <div class="auth-head">
    <img src="/logo.png" alt="OvalBot" class="auth-logo" />
    <h1>Create your account</h1>
    <p class="muted">OvalBot is limited to 10 active members.</p>
  </div>

  <form class="card" onsubmit={onSubmit}>
    {#if error}
      <div class="alert alert-error">{error}</div>
    {/if}

    <div class="field">
      <label for="email">Email</label>
      <input
        id="email"
        class="input"
        type="email"
        autocomplete="email"
        required
        bind:value={email}
      />
    </div>

    <div class="field">
      <label for="password">Password</label>
      <input
        id="password"
        class="input"
        type="password"
        autocomplete="new-password"
        minlength="8"
        required
        bind:value={password}
      />
      <p class="hint">At least 8 characters.</p>
    </div>

    <button class="btn btn-primary btn-block" type="submit" disabled={loading}>
      {loading ? 'Creating account…' : 'Sign up'}
    </button>

    <p class="muted center">Already have an account? <a href="/login">Sign in</a></p>
  </form>
</div>
