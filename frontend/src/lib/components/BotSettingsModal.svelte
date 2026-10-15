<script lang="ts">
  let dialogEl: HTMLDialogElement;

  let searchDuo = $state(false);
  let searchSingle = $state(false);
  let searchGroup = $state(0);

  export function open() {
    dialogEl?.showModal();
  }

  function close() {
    dialogEl?.close();
  }

  function save() {
    dialogEl?.close();
  }
</script>

<dialog bind:this={dialogEl} class="modal">
  <div class="modal__head">
    <h2>Bot settings</h2>
    <button class="modal__close" type="button" aria-label="Close" onclick={close}>&times;</button>
  </div>

  <div class="field">
    <label class="switch">
      <input type="checkbox" bind:checked={searchDuo} />
      <span>Search for duo tickets</span>
    </label>
    <label class="switch">
      <input type="checkbox" bind:checked={searchSingle} />
      <span>Search for single tickets</span>
    </label>
  </div>

  <div class="field">
    <label for="group-tickets">Search group tickets</label>
    <input id="group-tickets" class="input" type="number" min="0" bind:value={searchGroup} />
  </div>

  <div class="modal__actions">
    <button class="btn btn-secondary" type="button" onclick={close}>Close</button>
    <button class="btn btn-primary" type="button" onclick={save}>Save</button>
  </div>
</dialog>
