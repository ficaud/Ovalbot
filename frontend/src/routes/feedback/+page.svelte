<script lang="ts">
  import { submitFeedback } from '#lib/api';
  import { formatDate } from '#lib/format';
  import type { FeedbackKind, FeedbackReportView } from '#lib/types';
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();

  const MAX_CHARS = 4000;

  let kind = $state<FeedbackKind>('bug');
  let message = $state('');
  let busy = $state(false);
  let error = $state('');
  let submitted = $state<FeedbackReportView | null>(null);

  const charCount = $derived(message.length);
  const validationError = $derived.by(() => {
    if (!message.trim()) return 'Describe the bug or the suggestion.';
    if (charCount > MAX_CHARS) return `Keep it under ${MAX_CHARS} characters.`;
    return '';
  });
  const canSubmit = $derived(!!validationError || busy);

  async function save(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    error = '';
    try {
      submitted = await submitFeedback({ kind, message });
      message = '';
    } catch (err) {
      error = err instanceof Error ? err.message : 'Something went wrong';
    } finally {
      busy = false;
    }
  }
</script>

<section class="page">
  <div class="card">
    <h2>Report a bug</h2>
    <p class="muted">
      Something broken, or an idea to make OvalBot better? Tell us — the report
      reaches the maintainers by email.
    </p>

    {#if submitted}
      <div class="alert alert-success">
        Thanks! Your {submitted.kind} report was sent (#{submitted.id}) on {formatDate(submitted.created_at)}.
      </div>
    {/if}

    {#if error}
      <div class="alert alert-error">{error}</div>
    {/if}

    <form onsubmit={save}>
      <fieldset class="field" disabled={busy}>
        <label for="feedback-kind">Type</label>
        <div class="radio-row">
          <label class="radio-option">
            <input type="radio" bind:group={kind} value="bug" name="feedback-kind" />
            Bug report
          </label>
          <label class="radio-option">
            <input type="radio" bind:group={kind} value="suggestion" name="feedback-kind" />
            Suggestion
          </label>
        </div>
      </fieldset>

      <div class="field">
        <label for="feedback-message">Message</label>
        <textarea
          id="feedback-message"
          class="input"
          rows="6"
          placeholder="What happened, and what did you expect? For a suggestion, what would you like to see?"
          bind:value={message}
          disabled={busy}
        ></textarea>
        <span class="hint {charCount > MAX_CHARS ? 'hint-error' : ''}">
          {charCount}/{MAX_CHARS}
        </span>
      </div>

      <button type="submit" class="btn btn-primary" disabled={canSubmit}>
        {busy ? 'Sending…' : 'Send report'}
      </button>
    </form>
  </div>
</section>

<style>
  .radio-row {
    display: flex;
    gap: 16px;
  }

  .radio-option {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: normal;
    cursor: pointer;
  }

  .hint-error {
    color: var(--danger, #b91c1c);
  }
</style>
