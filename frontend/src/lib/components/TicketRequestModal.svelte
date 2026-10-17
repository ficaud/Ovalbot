<script lang="ts">
  import type { MatchView, TicketKind, TicketRequestDraft } from '#lib/types';

  const MAX_SINGLE = 10;
  const MIN_GROUP = 2;
  const MAX_GROUP = 10;

  let {
    onsave,
    onremove
  }: {
    onsave: (draft: TicketRequestDraft) => Promise<void>;
    onremove: (matchId: number) => Promise<void>;
  } = $props();

  let dialogEl: HTMLDialogElement;

  let match = $state<MatchView | null>(null);
  let hasExisting = $state(false);
  let kind = $state<TicketKind>('single');
  let singleQuantity = $state<number | undefined>(1);
  let singlePrice = $state<number | undefined>(undefined);
  let duoPrice = $state<number | undefined>(undefined);
  let groupPlaces = $state<number | undefined>(4);
  let saving = $state(false);
  let removing = $state(false);
  let serverError = $state('');

  let error = $derived.by(() => {
    if (kind === 'single') {
      if (!inRange(singleQuantity, 1, MAX_SINGLE)) {
        return `Number of tickets must be between 1 and ${MAX_SINGLE}.`;
      }
      return priceError(singlePrice, 'per ticket');
    }
    if (kind === 'duo') {
      return priceError(duoPrice, 'for the pair');
    }
    if (!inRange(groupPlaces, MIN_GROUP, MAX_GROUP)) {
      return `Number of places must be between ${MIN_GROUP} and ${MAX_GROUP}.`;
    }
    return '';
  });

  function inRange(value: number | undefined, min: number, max: number): boolean {
    return value !== undefined && Number.isInteger(value) && value >= min && value <= max;
  }

  function validPrice(value: number | undefined): value is number {
    return value !== undefined && Number.isFinite(value) && value > 0;
  }

  function priceError(value: number | undefined, suffix: string): string {
    return validPrice(value) ? '' : `Enter a maximum price ${suffix}.`;
  }

  function toCents(value: number | undefined): number | null {
    return validPrice(value) ? Math.round(value * 100) : null;
  }

  function toEuros(cents: number | null): number | undefined {
    return cents === null ? undefined : cents / 100;
  }

  export function open(selected: MatchView, draft: TicketRequestDraft | null) {
    match = selected;
    hasExisting = draft !== null;
    kind = draft?.kind ?? 'single';
    singleQuantity = draft?.single.quantity ?? 1;
    singlePrice = toEuros(draft?.single.maxPriceCents ?? null);
    duoPrice = toEuros(draft?.duo.maxPriceCents ?? null);
    groupPlaces = draft?.group.places ?? 4;
    saving = false;
    removing = false;
    serverError = '';
    dialogEl?.showModal();
  }

  function close() {
    dialogEl?.close();
  }

  async function save() {
    if (!match || error !== '' || saving || removing) return;
    saving = true;
    serverError = '';
    try {
      await onsave({
        matchId: match.id,
        kind,
        single: {
          quantity: inRange(singleQuantity, 1, MAX_SINGLE) ? singleQuantity! : 1,
          maxPriceCents: toCents(singlePrice)
        },
        duo: { maxPriceCents: toCents(duoPrice) },
        group: {
          places: inRange(groupPlaces, MIN_GROUP, MAX_GROUP) ? groupPlaces! : MIN_GROUP
        }
      });
      close();
    } catch (err) {
      serverError = err instanceof Error ? err.message : 'Failed to send ticket request';
    } finally {
      saving = false;
    }
  }

  async function remove() {
    if (!match || saving || removing) return;
    removing = true;
    serverError = '';
    try {
      await onremove(match.id);
      close();
    } catch (err) {
      serverError = err instanceof Error ? err.message : 'Failed to remove ticket request';
    } finally {
      removing = false;
    }
  }
</script>

<dialog bind:this={dialogEl} class="modal">
  <div class="modal__head">
    <h2>Ticket request</h2>
    <button class="modal__close" type="button" aria-label="Close" onclick={close}>&times;</button>
  </div>

  {#if match}
    <p class="muted">For {match.team} vs {match.opponent} — {match.kickoff}</p>
  {/if}

  {#if serverError}
    <div class="alert alert-error">{serverError}</div>
  {/if}

  <fieldset class="options">
    <legend>Choose one option for this match.</legend>

    <div class="option" class:option--active={kind === 'single'}>
      <label class="option__head">
        <input type="radio" name="ticket-kind" value="single" bind:group={kind} />
        <span>Single tickets</span>
      </label>
      <div class="option__fields" class:option__fields--off={kind !== 'single'}>
        <label class="option__field">
          <span>Number of tickets (1–{MAX_SINGLE})</span>
          <input
            class="input"
            type="number"
            min="1"
            max={MAX_SINGLE}
            step="1"
            bind:value={singleQuantity}
            disabled={kind !== 'single'}
          />
        </label>
        <label class="option__field">
          <span>Max price per ticket</span>
          <span class="price">
            <input
              class="input"
              type="number"
              min="0"
              step="0.01"
              inputmode="decimal"
              bind:value={singlePrice}
              disabled={kind !== 'single'}
            />
            <span class="price__unit">€</span>
          </span>
        </label>
        {#if kind === 'single' && inRange(singleQuantity, 1, MAX_SINGLE)}
          <p class="hint">
            Up to {singleQuantity} ticket{singleQuantity === 1 ? '' : 's'}, bought as they become
            available.
          </p>
        {/if}
        {#if kind === 'single' && error}
          <p class="hint hint--warn">{error}</p>
        {/if}
      </div>
    </div>

    <div class="option" class:option--active={kind === 'duo'}>
      <label class="option__head">
        <input type="radio" name="ticket-kind" value="duo" bind:group={kind} />
        <span>Duo (2 seats)</span>
      </label>
      <div class="option__fields" class:option__fields--off={kind !== 'duo'}>
        <label class="option__field">
          <span>Max price for the pair</span>
          <span class="price">
            <input
              class="input"
              type="number"
              min="0"
              step="0.01"
              inputmode="decimal"
              bind:value={duoPrice}
              disabled={kind !== 'duo'}
            />
            <span class="price__unit">€</span>
          </span>
        </label>
        {#if kind === 'duo' && error}
          <p class="hint hint--warn">{error}</p>
        {/if}
      </div>
    </div>

    <div class="option" class:option--active={kind === 'group'}>
      <label class="option__head">
        <input type="radio" name="ticket-kind" value="group" bind:group={kind} />
        <span>Group tickets — grass area only (pelouse)</span>
      </label>
      <div class="option__fields" class:option__fields--off={kind !== 'group'}>
        <label class="option__field">
          <span>Number of places ({MIN_GROUP}–{MAX_GROUP})</span>
          <input
            class="input"
            type="number"
            min={MIN_GROUP}
            max={MAX_GROUP}
            step="1"
            bind:value={groupPlaces}
            disabled={kind !== 'group'}
          />
        </label>
        <p class="hint">
          No assigned seat: the group can stand together. The standard pelouse price applies.
        </p>
        {#if kind === 'group' && error}
          <p class="hint hint--warn">{error}</p>
        {/if}
      </div>
    </div>
  </fieldset>

  <div class="actions">
    {#if hasExisting}
      <button
        class="btn btn-danger"
        type="button"
        onclick={remove}
        disabled={saving || removing}
      >
        {removing ? 'Removing…' : 'Remove request'}
      </button>
    {/if}
    <div class="actions__end">
      <button class="btn btn-secondary" type="button" onclick={close} disabled={saving || removing}>
        Cancel
      </button>
      <button
        class="btn btn-primary"
        type="button"
        onclick={save}
        disabled={error !== '' || saving || removing}
      >
        {saving ? 'Saving…' : 'Save request'}
      </button>
    </div>
  </div>
</dialog>

<style>
  .options {
    border: none;
    padding: 0;
    margin: 16px 0 0;
  }

  .options legend {
    font-weight: 600;
    font-size: 0.9rem;
    padding: 0;
    margin-bottom: 10px;
  }

  .option {
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 12px 14px;
    margin-bottom: 10px;
    transition: border-color 0.15s ease;
  }

  .option--active {
    border-color: var(--yellow);
    background: #fffdf0;
  }

  .option__head {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    font-weight: 600;
  }

  .option__fields {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-top: 12px;
  }

  .option__fields--off .option__field > span:first-child {
    opacity: 0.5;
  }

  .option__field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .option__field > span:first-child {
    font-size: 0.9rem;
    font-weight: 600;
  }

  .price {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .price__unit {
    color: var(--muted);
    font-weight: 600;
  }

  .hint--warn {
    color: var(--danger);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 20px;
  }

  .actions__end {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
</style>
