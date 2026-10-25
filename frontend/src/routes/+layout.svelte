<script lang="ts">
  import { goto, invalidateAll } from '$app/navigation';
  import { logout } from '#lib/api';
  import type { LayoutData } from './$types';
  import type { Snippet } from 'svelte';
  import '../app.css';

  let { data, children }: { data: LayoutData; children: Snippet } = $props();

  async function onLogout() {
    await logout().catch(() => {});
    await invalidateAll();
    await goto('/login');
  }
</script>

<div class="shell">
  <header class="site-header">
    <div class="bar">
      <a href="/" class="brand">
        <img src="/logo.png" alt="OvalBot" class="brand-logo" />
        <span class="brand-name">OvalBot</span>
      </a>

      <nav class="nav">
        {#if data.user}
          {#if data.user.role === 'admin'}
            <a href="/admin" class="nav-link">Admin</a>
          {/if}
          <span class="nav-email">{data.user.email}</span>
          <button class="btn btn-ghost btn-sm" onclick={onLogout}>Log out</button>
        {:else}
          <a href="/login" class="nav-link">Sign in</a>
          <a href="/register" class="btn btn-primary btn-sm">Sign up</a>
        {/if}
      </nav>
    </div>
  </header>

  <main class="site-main">
    {@render children()}
  </main>

  <footer class="site-footer">
    <nav class="footer-links" aria-label="Legal pages">
      <a href="/legal-notice">Legal notice</a>
      <a href="/privacy">Privacy policy</a>
      <a href="/terms">Terms of use</a>
      {#if data.version}
        <span class="footer-version"><a href="https://github.com/ficaud/Ovalbot/releases/">v{data.version.version}</a></span>
      {/if}
    </nav>
  </footer>
</div>
