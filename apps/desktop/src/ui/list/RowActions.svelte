<script lang="ts">
  // A row's own buttons, shown while the pointer is on the row, and on the keyboard row as a hint of what it offers. They are for the pointer: the keyboard reaches the same actions through the list's keys and its context menu, so they take no place in the tab order. They sit on a solid surface over the row's last figures, which give way to them.
  import FolderOpen from '~icons/lucide/folder-open';
  import PanelRight from '~icons/lucide/panel-right';
  import Pause from '~icons/lucide/pause';
  import Play from '~icons/lucide/play';
  import RotateCw from '~icons/lucide/rotate-cw';
  import SquareArrowOutUpRight from '~icons/lucide/square-arrow-out-up-right';
  import Trash from '~icons/lucide/trash-2';
  import type { DownloadSummary } from '@wirecatch/protocol';
  import { cn, IconButton } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';
  import { canOpen, canPause, canResume, canRetry } from '$lib/state/row-actions';

  let { row, active }: { row: DownloadSummary; active: boolean } = $props();

  const app = getApp();
  const ids = $derived([row.id]);
</script>

<div
  class={cn(
    'absolute top-1/2 right-2 flex -translate-y-1/2 items-center gap-0.5 rounded-md border border-line bg-base p-0.5 opacity-0 shadow-raised transition-opacity group-hover:opacity-100',
    active && 'group-focus-visible/list:opacity-100',
  )}
>
  {#if canPause(row)}
    <IconButton
      icon={Pause}
      label="Pause"
      size="sm"
      tabindex={-1}
      onclick={() => app.actions.pause(ids)}
    />
  {:else if canResume(row)}
    <IconButton
      icon={Play}
      label="Resume"
      size="sm"
      tabindex={-1}
      onclick={() => app.actions.resume(ids)}
    />
  {:else if canRetry(row) && row.error?.fix !== 'retry'}
    <!-- A failure whose fix is to try again shows that fix in the row already. -->
    <IconButton
      icon={RotateCw}
      label={row.status === 'cancelled' ? 'Start over' : 'Try again'}
      size="sm"
      tabindex={-1}
      onclick={() => app.actions.retry(ids)}
    />
  {/if}
  {#if canOpen(row)}
    <IconButton
      icon={SquareArrowOutUpRight}
      label="Open"
      size="sm"
      tabindex={-1}
      onclick={() => app.actions.openFile(row.id)}
    />
  {/if}
  <IconButton
    icon={FolderOpen}
    label="Show in folder"
    size="sm"
    tabindex={-1}
    onclick={() => app.actions.openFolder(row.id)}
  />
  <IconButton
    icon={PanelRight}
    label="Show details"
    size="sm"
    tabindex={-1}
    onclick={() => app.openDetails(row.id)}
  />
  <IconButton
    icon={Trash}
    label="Remove"
    size="sm"
    tabindex={-1}
    onclick={() => app.openDialog({ kind: 'remove', ids })}
  />
</div>
