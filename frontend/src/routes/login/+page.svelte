<script lang="ts">
  import { goto, invalidateAll } from '$app/navigation';
  import { page } from '$app/state';
  import { login } from '#lib/api';

  let email = $state('');
  let password = $state('');
  let error = $state('');
  let loading = $state(false);

  let notice = $derived(page.url.searchParams.get('registered'));

  async function onSubmit(e: SubmitEvent) {
    e.preventDefault();
    error = '';
    loading = true;
    try {
      await login(email, password);
      await invalidateAll();
      await goto('/');
    } catch (err) {
      error = err instanceof Error ? err.message : 'Something went wrong';
    } finally {
      loading = false;
    }
  }
</script>

<div class="auth-shell">
  <div class="auth-head">
    <img src="/logo.png" alt="OvalBot" class="auth-logo" />
    <h1>Welcome back</h1>
    <p class="muted">Sign in to access the match calendar.</p>
  </div>

  <form class="card" onsubmit={onSubmit}>
    {#if notice === 'active'}
      <div class="alert alert-success">Account created — you can sign in now.</div>
    {:else if notice === 'waiting'}
      <div class="alert alert-info">You're on the waitlist — sign in to check your status.</div>
    {/if}

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
        autocomplete="current-password"
        required
        bind:value={password}
      />
    </div>

    <button class="btn btn-primary btn-block" type="submit" disabled={loading}>
      {loading ? 'Signing in…' : 'Sign in'}
    </button>

    <p class="muted center">No account yet? <a href="/register">Sign up</a></p>
  </form>
</div>
