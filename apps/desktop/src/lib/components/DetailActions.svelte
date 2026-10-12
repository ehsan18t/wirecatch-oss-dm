<script lang="ts">
  // What the detail panel offers to do with the selection: one download's own buttons, or what
  // applies to several.
  import AppWindow from '@lucide/svelte/icons/app-window';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import Pause from '@lucide/svelte/icons/pause';
  import Play from '@lucide/svelte/icons/play';
  import RotateCw from '@lucide/svelte/icons/rotate-cw';
  import Trash from '@lucide/svelte/icons/trash-2';
  import type { DownloadSummary } from '@wirecatch/protocol';
  import { getApp } from '$lib/state/app.svelte';
  import { isUnderWay } from '$lib/state/view';
  import Button from './Button.svelte';

  let { rows }: { rows: readonly DownloadSummary[] } = $props();

  const app = getApp();

  const one = $derived(rows.length === 1 ? rows[0] : undefined);
  const anyRunning = $derived(rows.some((r) => isUnderWay(r.status)));
  const anyStopped = $derived(rows.some((r) => r.status === 'paused' || r.status === 'failed'));
  const anyFailed = $derived(rows.some((r) => r.status === 'failed' || r.status === 'cancelled'));
</script>

<div class="actions">
  {#if one}
    {#if isUnderWay(one.status)}
      <Button icon={Pause} onclick={() => app.actions.pause([one.id])}>Pause</Button>
    {:else if one.status === 'paused'}
      <Button variant="primary" icon={Play} onclick={() => app.actions.resume([one.id])}
        >Resume</Button
      >
    {:else if one.status === 'cancelled'}
      <Button variant="primary" icon={RotateCw} onclick={() => app.actions.retry([one.id])}>
        Start over</Button
      >
    {:else if one.status === 'failed' && !one.error}
      <!-- With an error, the error box above already offers Try again. -->
      <Button icon={RotateCw} onclick={() => app.actions.retry([one.id])}>Try again</Button>
    {/if}
    {#if one.status === 'completed'}
      <Button variant="primary" onclick={() => app.actions.openFile(one.id)}>Open</Button>
    {/if}
    <Button icon={FolderOpen} onclick={() => app.actions.openFolder(one.id)}>Show in folder</Button>
    <Button icon={AppWindow} onclick={() => app.showProgressWindow(one.id)}>
      Show progress window</Button
    >
    <Button icon={Trash} onclick={() => app.openDialog({ kind: 'remove', ids: [one.id] })}>
      Remove
    </Button>
  {:else}
    {#if anyRunning}
      <Button icon={Pause} onclick={() => app.actions.pause()}>Pause</Button>
    {/if}
    {#if anyStopped}
      <Button icon={Play} onclick={() => app.actions.resume()}>Resume</Button>
    {/if}
    {#if anyFailed}
      <Button icon={RotateCw} onclick={() => app.actions.retry()}>Try again</Button>
    {/if}
    <Button
      icon={Trash}
      onclick={() => app.openDialog({ kind: 'remove', ids: app.selection.list })}
    >
      Remove
    </Button>
  {/if}
</div>

<style>
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
</style>
