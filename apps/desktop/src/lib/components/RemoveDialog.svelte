<script lang="ts">
  // Removing downloads from the list: what happens to their files, and, when some are finished, whether those files go too.
  import type { DownloadId } from '@wirecatch/protocol';
  import { Button, Checkbox, Dialog } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';

  let { ids }: { ids: DownloadId[] } = $props();
  const app = getApp();

  let deleteFiles = $state(false);
  const rows = $derived(ids.flatMap((id) => app.downloads.get(id) ?? []));
  const finished = $derived(rows.filter((r) => r.status === 'completed').length);
  const unfinished = $derived(rows.length - finished);
  const one = $derived(rows.length === 1 ? rows[0] : undefined);

  async function remove() {
    await app.actions.remove(ids, deleteFiles && finished > 0);
    app.closeDialog();
  }
</script>

<Dialog
  open
  role="alertdialog"
  title={one ? 'Remove this download?' : `Remove ${String(rows.length)} downloads?`}
  onclose={() => app.closeDialog()}
>
  <div class="flex flex-col gap-4">
    {#if one}
      <p class="text-ui font-medium break-all text-ink">{one.file_name}</p>
    {/if}
    {#if unfinished > 0}
      <p class="text-ui text-ink-muted">
        {unfinished === rows.length
          ? rows.length === 1
            ? 'It stops, and the part already downloaded is deleted.'
            : 'They stop, and the parts already downloaded are deleted.'
          : `${String(unfinished)} of them are unfinished: they stop, and their partial files are deleted.`}
      </p>
    {/if}
    {#if finished > 0}
      <Checkbox
        label={finished === 1
          ? 'Also delete the file from the disk'
          : 'Also delete the finished files from the disk'}
        checked={deleteFiles}
        onchange={(on: boolean) => (deleteFiles = on)}
      />
    {/if}
  </div>

  {#snippet footer()}
    <Button data-autofocus onclick={() => app.closeDialog()}>Keep</Button>
    <Button variant="danger" onclick={remove}>
      {deleteFiles && finished > 0 ? 'Remove and delete' : 'Remove from list'}
    </Button>
  {/snippet}
</Dialog>
