<script lang="ts">
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();
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
      <h2>Welcome, {data.user.email}</h2>
      <p class="muted">
        The home match calendar is on its way. Nothing to see here yet.
      </p>
    </div>

    {#if data.user.role === 'admin'}
      <div class="card">
        <h3>Administration</h3>
        <p class="muted">Manage accounts and the waitlist.</p>
        <a href="/admin" class="btn btn-secondary">Manage accounts</a>
      </div>
    {/if}
  {/if}
</section>
