<script lang="ts">
  import { getApp } from '$lib/state/app.svelte';
  import AddDialog from './AddDialog.svelte';
  import CancelDialog from './CancelDialog.svelte';
  import QueueDialog from './QueueDialog.svelte';
  import RefreshDialog from './RefreshDialog.svelte';
  import RemoveDialog from './RemoveDialog.svelte';
  import ShortcutsDialog from './ShortcutsDialog.svelte';
  import TransferDialog from './TransferDialog.svelte';

  const app = getApp();
</script>

{#if app.dialog}
  {@const d = app.dialog}
  {#key d}
    {#if d.kind === 'add'}
      <AddDialog urls={d.urls} queue={d.queue} />
    {:else if d.kind === 'remove'}
      <RemoveDialog ids={d.ids} />
    {:else if d.kind === 'cancel'}
      <CancelDialog ids={d.ids} />
    {:else if d.kind === 'queue'}
      <QueueDialog id={d.id} />
    {:else if d.kind === 'refresh'}
      <RefreshDialog id={d.id} />
    {:else if d.kind === 'export' || d.kind === 'import'}
      <TransferDialog mode={d.kind} />
    {:else if d.kind === 'shortcuts'}
      <ShortcutsDialog />
    {/if}
  {/key}
{/if}
