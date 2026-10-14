<script lang="ts">
  // One queue in the sidebar: what it is doing, and its own buttons (edit, pause or resume, start now outside its hours). Downloads dragged onto it move to it.
  import ListOrdered from '~icons/lucide/list-ordered';
  import Pause from '~icons/lucide/pause';
  import Pencil from '~icons/lucide/pencil';
  import Play from '~icons/lucide/play';
  import type { Queue } from '@wirecatch/protocol';
  import { formatSpeed } from '@wirecatch/units';
  import { cn, IconButton, SidebarItem } from '@wirecatch/ui';
  import { getDragIds, isDownloadDrag } from '$lib/dnd';
  import { getApp } from '$lib/state/app.svelte';

  let { queue }: { queue: Queue } = $props();

  const app = getApp();

  let dropping = $state(false);

  const shown = $derived(
    app.screen === 'downloads' && app.view.kind === 'queue' && app.view.id === queue.id,
  );
  const speed = $derived(app.downloads.queueSpeed[queue.id] ?? 0);
  const count = $derived(app.counts.queue.get(queue.id)?.total ?? 0);
  const outside = $derived(app.queues.isOutsideSchedule(queue) && !queue.paused);

  function ondragover(e: DragEvent) {
    if (!isDownloadDrag(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    dropping = true;
  }

  function ondrop(e: DragEvent) {
    if (!isDownloadDrag(e)) return;
    e.preventDefault();
    e.stopPropagation();
    dropping = false;
    const ids = getDragIds(e);
    if (ids.length > 0) void app.actions.moveToQueue(ids, queue.id);
  }
</script>

<div
  role="presentation"
  class={cn('rounded-sm', dropping && 'bg-accent-soft outline-2 -outline-offset-2 outline-accent')}
  {ondragover}
  ondragleave={() => (dropping = false)}
  {ondrop}
>
  <SidebarItem
    label={queue.name}
    icon={ListOrdered}
    active={shown}
    count={speed > 0 ? undefined : count}
    description={app.queues.status(queue)}
    onclick={() => app.show({ kind: 'queue', id: queue.id })}
  >
    {#snippet trailing()}
      {#if speed > 0}
        <span class="text-small text-accent tabular-nums">{formatSpeed(speed)}</span>
      {:else if count > 0}
        <span class="text-small text-ink-muted tabular-nums">{count}</span>
      {/if}
    {/snippet}
    {#snippet actions()}
      {#if outside}
        <IconButton
          icon={Play}
          label="Start {queue.name} now, outside its hours"
          tooltip="Start now"
          size="xs"
          onclick={() => app.queues.startNow(queue.id)}
        />
      {/if}
      <IconButton
        icon={queue.paused ? Play : Pause}
        label={queue.paused ? `Resume the ${queue.name} queue` : `Pause the ${queue.name} queue`}
        tooltip={queue.paused ? 'Resume this queue' : 'Pause this queue'}
        size="xs"
        onclick={() => app.queues.setPaused(queue.id, !queue.paused)}
      />
      <IconButton
        icon={Pencil}
        label="Edit {queue.name}: name, schedule and limits"
        tooltip="Edit name, schedule and limits"
        size="xs"
        onclick={() => app.openDialog({ kind: 'queue', id: queue.id })}
      />
    {/snippet}
  </SidebarItem>
</div>
