<script lang="ts">
  // One queue in the sidebar: what it is doing, and one button for its own actions (start now outside its hours, pause or resume, edit), which its view's header shows too. Downloads dragged onto it move to it.
  import Ellipsis from '~icons/lucide/ellipsis';
  import ListOrdered from '~icons/lucide/list-ordered';
  import Pause from '~icons/lucide/pause';
  import Pencil from '~icons/lucide/pencil';
  import Play from '~icons/lucide/play';
  import type { Queue } from '@wirecatch/protocol';
  import { formatSpeed } from '@wirecatch/units';
  import {
    cn,
    IconButton,
    Menu,
    SidebarItem,
    type MenuEntry,
    type MenuTriggerProps,
  } from '@wirecatch/ui';
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

  const entries = $derived<MenuEntry[]>([
    ...(outside
      ? [
          {
            id: 'start',
            label: 'Start now, outside its hours',
            icon: Play,
            onselect: () => void app.queues.startNow(queue.id),
          },
        ]
      : []),
    {
      id: 'pause',
      label: queue.paused ? 'Resume the queue' : 'Pause the queue',
      icon: queue.paused ? Play : Pause,
      onselect: () => void app.queues.setPaused(queue.id, !queue.paused),
    },
    {
      id: 'edit',
      label: 'Edit name, hours and limits',
      icon: Pencil,
      onselect: () => app.openDialog({ kind: 'queue', id: queue.id }),
    },
  ]);

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
      <Menu {entries} label="{queue.name} queue" align="start">
        {#snippet trigger(props: MenuTriggerProps)}
          <IconButton {...props} icon={Ellipsis} label="Actions for {queue.name}" size="xs" />
        {/snippet}
      </Menu>
    {/snippet}
  </SidebarItem>
</div>
