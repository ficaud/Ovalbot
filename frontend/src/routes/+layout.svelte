<script lang="ts">
  import { goto, invalidateAll } from '$app/navigation';
  import { logout } from '$lib/api';
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

  {#if data.version}
    <span class="version">v{data.version.version}</span>
  {/if}
</div>
