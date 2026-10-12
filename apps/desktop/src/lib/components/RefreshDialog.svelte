<script lang="ts">
  import type { DownloadId } from '@wirecatch/protocol';
  import { extractUrls, hostOf } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';

  let { id }: { id: DownloadId } = $props();
  const app = getApp();

  const row = $derived(app.downloads.get(id));
  let url = $state('');
  let error = $state('');

  async function save() {
    const found = extractUrls(url)[0];
    if (!found) {
      error = 'Paste the full new link, starting with https://.';
      return;
    }
    if (await app.refreshLink(id, found)) {
      app.toasts.notify('New link saved. The download continues where it stopped.');
      app.closeDialog();
    }
  }
</script>

<Dialog
  title="Give it a fresh link"
  description="Links from some sites stop working after a while. Start the download again on the page it came from, copy the new link and paste it here. Everything already downloaded is kept."
  onclose={() => app.closeDialog()}
  size="wide"
>
  <form
    id="refresh-form"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    {#if row}
      <p class="file">
        <span class="name">{row.file_name}</span>
        <span class="from">from {hostOf(row.url)}</span>
      </p>
    {/if}
    <label class="form-stack">
      <span class="form-label">New link</span>
      <input
        class="field"
        type="url"
        placeholder="https://"
        aria-invalid={error ? 'true' : undefined}
        bind:value={url}
        oninput={() => (error = '')}
      />
    </label>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </form>
  {#snippet footer()}
    <Button onclick={() => app.closeDialog()}>Cancel</Button>
    <Button variant="primary" type="submit" form="refresh-form">Use this link</Button>
  {/snippet}
</Dialog>

<style>
  form {
    display: grid;
    gap: 12px;
  }

  .file {
    display: grid;
    margin: 0;
  }

  .name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .from,
  .error {
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .error {
    margin: 0;
    color: var(--danger);
  }
</style>
