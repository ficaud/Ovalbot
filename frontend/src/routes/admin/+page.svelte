<script lang="ts">
  import { invalidate } from '$app/navigation';
  import { activateUser, deleteUser } from '#lib/api';
  import { formatDate } from '#lib/format';
  import type { UserView } from '#lib/types';
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();

  let error = $state('');
  let busy = $state<number | null>(null);

  const activeCount = $derived(data.users.filter((u) => u.status === 'active').length);

  async function activate(id: number) {
    busy = id;
    error = '';
    try {
      await activateUser(id);
      await invalidate('app:users');
    } catch (err) {
      error = err instanceof Error ? err.message : 'Something went wrong';
    } finally {
      busy = null;
    }
  }

  async function remove(user: UserView) {
    if (!confirm(`Delete ${user.email}?`)) return;
    busy = user.id;
    error = '';
    try {
      await deleteUser(user.id);
      await invalidate('app:users');
    } catch (err) {
      error = err instanceof Error ? err.message : 'Something went wrong';
    } finally {
      busy = null;
    }
  }
</script>

<section class="page">
  <div class="card">
    <h2>Accounts</h2>
    <p class="muted">
      {activeCount} active · {data.users.length} total
    </p>

    {#if error}
      <div class="alert alert-error">{error}</div>
    {/if}

    <div class="table-wrap">
      <table class="table">
        <thead>
          <tr>
            <th>Email</th>
            <th>Status</th>
            <th>Role</th>
            <th>Joined</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each data.users as user (user.id)}
            <tr>
              <td>{user.email}</td>
              <td><span class="badge badge-{user.status}">{user.status}</span></td>
              <td><span class="badge badge-{user.role}">{user.role}</span></td>
              <td>{formatDate(user.created_at)}</td>
              <td class="actions">
                {#if user.status === 'waiting'}
                  <button
                    class="btn btn-primary btn-sm"
                    onclick={() => activate(user.id)}
                    disabled={busy === user.id}
                  >
                    Activate
                  </button>
                {/if}
                {#if user.role !== 'admin'}
                  <button
                    class="btn btn-danger btn-sm"
                    onclick={() => remove(user)}
                    disabled={busy === user.id}
                  >
                    Delete
                  </button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>
</section>
