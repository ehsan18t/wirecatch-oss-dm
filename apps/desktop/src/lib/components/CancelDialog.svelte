<script lang="ts">
  import type { DownloadId } from '@wirecatch/protocol';
  import { getApp } from '$lib/state/app.svelte';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';

  let { ids }: { ids: DownloadId[] } = $props();
  const app = getApp();

  const rows = $derived(ids.flatMap((id) => app.downloads.get(id) ?? []));
  const title = $derived(
    rows.length === 1 ? `Cancel "${rows[0]?.file_name}"?` : `Cancel ${rows.length} downloads?`,
  );

  async function cancel() {
    // Read the ids before closing: closing unmounts this dialog and its props with it.
    const chosen = [...ids];
    app.closeDialog();
    await app.cancel(chosen);
  }
</script>

<Dialog {title} onclose={() => app.closeDialog()} size="narrow">
  <p class="text">
    This stops the download and deletes the part already downloaded. It stays in the list, so you
    can start it over later.
  </p>

  {#snippet footer()}
    <Button data-autofocus onclick={() => app.closeDialog()}>Keep downloading</Button>
    <Button variant="danger" onclick={cancel}>Cancel and delete the partial file</Button>
  {/snippet}
</Dialog>

<style>
  .text {
    margin: 0;
    color: var(--ink-2);
    line-height: 1.5;
  }
</style>
