<script lang="ts">
  // Cancelling downloads: they stop and lose what was downloaded, but stay in the list to start over.
  import type { DownloadId } from '@wirecatch/protocol';
  import { Button, Dialog } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';

  let { ids }: { ids: DownloadId[] } = $props();
  const app = getApp();

  const rows = $derived(ids.flatMap((id) => app.downloads.get(id) ?? []));
  const one = $derived(rows.length === 1 ? rows[0] : undefined);
  /** Some of it is on the disk already, and goes. */
  const started = $derived(rows.some((r) => r.done > 0));
  const running = $derived(
    rows.some((r) => r.status === 'downloading' || r.status === 'connecting'),
  );

  async function cancel() {
    // Read the ids before closing: closing unmounts this dialog and its props with it.
    const chosen = [...ids];
    app.closeDialog();
    await app.actions.cancel(chosen);
  }
</script>

<Dialog
  open
  role="alertdialog"
  title={one ? 'Cancel this download?' : `Cancel ${String(rows.length)} downloads?`}
  onclose={() => app.closeDialog()}
>
  <div class="flex flex-col gap-4">
    {#if one}
      <p class="text-ui font-medium break-all text-ink">{one.file_name}</p>
    {/if}
    <p class="text-ui text-ink-muted">
      {#if one}
        {started ? 'It stops, and the part already downloaded is deleted.' : 'It will not start.'}
        It stays in the list, so you can start it over later.
      {:else}
        {started
          ? 'They stop, and the parts already downloaded are deleted.'
          : 'They will not start.'}
        They stay in the list, so you can start them over later.
      {/if}
    </p>
  </div>

  {#snippet footer()}
    <Button data-autofocus onclick={() => app.closeDialog()}
      >{running ? 'Keep downloading' : one ? 'Keep it' : 'Keep them'}</Button
    >
    <Button variant="danger" onclick={cancel}>Cancel download</Button>
  {/snippet}
</Dialog>
