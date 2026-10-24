<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    title,
    updated,
    children
  }: {
    title: string;
    updated?: string;
    children: Snippet;
  } = $props();
</script>

<article class="card legal">
  <h1>{title}</h1>
  {#if updated}
    <p class="legal-updated muted">Last updated: {updated}</p>
  {/if}
  {@render children()}
</article>

<style>
  .legal {
    max-width: 720px;
  }

  .legal h1 {
    margin-bottom: 4px;
  }

  .legal-updated {
    font-size: 0.8rem;
    margin-bottom: 16px;
  }

  .legal :global(h2) {
    margin-top: 24px;
    font-size: 1.15rem;
    border-bottom: 1px solid var(--border, #e5e7eb);
    padding-bottom: 6px;
  }

  .legal :global(ul) {
    margin: 0 0 12px;
    padding-left: 22px;
  }

  .legal :global(li) {
    margin-bottom: 4px;
  }

  .legal :global(dt) {
    font-weight: 700;
    margin-top: 12px;
  }

  .legal :global(dd) {
    margin: 2px 0 0;
  }

  .legal :global(mark.placeholder) {
    background: var(--yellow, #ffd500);
    color: var(--black, #0b0b0b);
    padding: 1px 5px;
    border-radius: 6px;
    font-weight: 600;
  }
</style>
