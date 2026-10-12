<script lang="ts">
  import type { DownloadId } from '@wirecatch/protocol';
  import { getApp } from '$lib/state/app.svelte';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';

  let { ids }: { ids: DownloadId[] } = $props();
  const app = getApp();

  let deleteFiles = $state(false);
  const rows = $derived(ids.flatMap((id) => app.downloads.get(id) ?? []));
  const unfinished = $derived(rows.filter((r) => r.status !== 'completed').length);
  const title = $derived(
    rows.length === 1 ? `Remove "${rows[0]?.file_name}"?` : `Remove ${rows.length} downloads?`,
  );

  async function remove() {
    await app.remove(ids, deleteFiles);
    app.closeDialog();
  }
</script>

<Dialog {title} onclose={() => app.closeDialog()} size="narrow">
  <p class="text">
    {rows.length === 1 ? 'It leaves the list.' : 'They leave the list.'}
    {#if unfinished > 0}
      {unfinished === rows.length
        ? 'Unfinished downloads stop and their partial files are deleted.'
        : `${unfinished} of them are unfinished; those stop and their partial files are deleted.`}
    {/if}
  </p>
  <label class="check">
    <input type="checkbox" bind:checked={deleteFiles} />
    <span>Also delete finished files from the disk</span>
  </label>

  {#snippet footer()}
    <Button data-autofocus onclick={() => app.closeDialog()}>Keep</Button>
    <Button variant="danger" onclick={remove}>
      {deleteFiles ? 'Remove and delete files' : 'Remove from list'}
    </Button>
  {/snippet}
</Dialog>

<style>
  .text {
    margin: 0 0 12px;
    color: var(--ink-2);
    line-height: 1.5;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .check input {
    width: 16px;
    height: 16px;
    accent-color: var(--accent);
  }
</style>
