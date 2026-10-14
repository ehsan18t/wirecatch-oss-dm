<script lang="ts">
  // What the details offer to do with the selection, under them, on one line: one download's own buttons, or what applies to several. The main action comes first and is the primary button; the window and Remove are icons at the end, so the line fits the panel.
  import AppWindow from '~icons/lucide/app-window';
  import FolderOpen from '~icons/lucide/folder-open';
  import Pause from '~icons/lucide/pause';
  import Play from '~icons/lucide/play';
  import RotateCw from '~icons/lucide/rotate-cw';
  import SquareArrowOutUpRight from '~icons/lucide/square-arrow-out-up-right';
  import Trash from '~icons/lucide/trash-2';
  import type { DownloadSummary } from '@wirecatch/protocol';
  import { Button, IconButton } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';
  import { applicable } from '$lib/state/row-actions';

  let { rows }: { rows: readonly DownloadSummary[] } = $props();

  const app = getApp();
  const ids = $derived(rows.map((r) => r.id));
  const one = $derived(rows.length === 1 ? rows[0] : undefined);
  const can = $derived(applicable(rows));
  // A failure with a fix offers it, with Try again, in its notice above.
  const retryHere = $derived(can.retry && !(one?.status === 'failed' && one.error));
</script>

{#if can.open && one}
  <Button
    variant="primary"
    icon={SquareArrowOutUpRight}
    onclick={() => app.actions.openFile(one.id)}>Open</Button
  >
{/if}
{#if can.pause}
  <Button icon={Pause} onclick={() => app.actions.pause(ids)}>Pause</Button>
{/if}
{#if can.resume}
  <Button
    variant={can.pause ? 'secondary' : 'primary'}
    icon={Play}
    onclick={() => app.actions.resume(ids)}>Resume</Button
  >
{/if}
{#if retryHere}
  <Button
    variant={one?.status === 'cancelled' ? 'primary' : 'secondary'}
    icon={RotateCw}
    onclick={() => app.actions.retry(ids)}
    >{one?.status === 'cancelled' ? 'Start over' : 'Try again'}</Button
  >
{/if}
{#if one}
  <Button icon={FolderOpen} onclick={() => app.actions.openFolder(one.id)}>Show in folder</Button>
{/if}
<span class="ml-auto flex items-center gap-1">
  {#if one}
    <IconButton
      icon={AppWindow}
      label="Show progress window"
      onclick={() => app.showProgressWindow(one.id)}
    />
  {/if}
  <IconButton
    icon={Trash}
    label={ids.length === 1 ? 'Remove' : `Remove ${String(ids.length)} downloads`}
    onclick={() => app.openDialog({ kind: 'remove', ids })}
  />
</span>
