<script lang="ts">
  // One queue in the sidebar: what it is doing, and one button for its own actions (start now outside its hours, pause or resume, edit, move up or down), which its view's header shows too. Downloads dragged onto it move to it; a queue below the default one can be dragged to another place (D72.9). It is marked Auto, dimmed with Auto off, and marked with the network icon while it uses a proxy (D72.4, D72.5).
  import ArrowDown from '~icons/lucide/arrow-down';
  import ArrowUp from '~icons/lucide/arrow-up';
  import Ellipsis from '~icons/lucide/ellipsis';
  import Network from '~icons/lucide/network';
  import ListOrdered from '~icons/lucide/list-ordered';
  import Pause from '~icons/lucide/pause';
  import Pencil from '~icons/lucide/pencil';
  import Play from '~icons/lucide/play';
  import type { Queue } from '@wirecatch/protocol';
  import { formatSpeed } from '@wirecatch/units';
  import {
    Badge,
    cn,
    Icon,
    IconButton,
    Menu,
    SidebarItem,
    textIfCut,
    tooltip,
    type MenuEntry,
    type MenuTriggerProps,
  } from '@wirecatch/ui';
  import {
    endQueueDrag,
    getDraggedQueue,
    getDragIds,
    isDownloadDrag,
    startQueueDrag,
  } from '$lib/dnd';
  import { getApp } from '$lib/state/app.svelte';

  let { queue }: { queue: Queue } = $props();

  const app = getApp();

  let dropping = $state(false);
  let dragging = $state(false);
  /** Where a dragged queue would land: above this one or below it. */
  let landing = $state<'above' | 'below' | null>(null);
  const index = $derived(app.queues.list.findIndex((q) => q.id === queue.id));
  const last = $derived(app.queues.list.length - 1);

  const shown = $derived(
    app.screen === 'downloads' && app.view.kind === 'queue' && app.view.id === queue.id,
  );
  const speed = $derived(app.downloads.queueSpeed[queue.id] ?? 0);
  const count = $derived(app.counts.queue.get(queue.id)?.total ?? 0);
  const takes = $derived(queue.settings.takes);
  // Until queues have proxies of their own (D73), each uses the app's, so all are marked while it is set by hand (D72.11).
  const proxied = $derived(app.settings?.proxy.mode === 'manual');
  const status = $derived(app.queues.describe(queue));
  const PROXY_NOTE = 'Uses a proxy';
  const outside = $derived(app.queues.isOutsideSchedule(queue) && !queue.status.paused);

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
      label: queue.status.paused ? 'Resume the queue' : 'Pause the queue',
      icon: queue.status.paused ? Play : Pause,
      onselect: () => void app.queues.setPaused(queue.id, !queue.status.paused),
    },
    {
      id: 'edit',
      label: 'Edit name, hours and limits',
      icon: Pencil,
      onselect: () => app.openDialog({ kind: 'queue', id: queue.id }),
    },
    ...(index > 0 && last > 1 ? [{ kind: 'separator' as const, id: 'order' }] : []),
    ...(index > 1
      ? [
          {
            id: 'up',
            label: 'Move up',
            icon: ArrowUp,
            onselect: () => void app.queues.moveTo(queue.id, index - 1),
          },
        ]
      : []),
    ...(index > 0 && index < last
      ? [
          {
            id: 'down',
            label: 'Move down',
            icon: ArrowDown,
            onselect: () => void app.queues.moveTo(queue.id, index + 1),
          },
        ]
      : []),
  ]);

  function ondragover(e: DragEvent) {
    const dragged = getDraggedQueue(e);
    if (dragged !== null) {
      const from = app.queues.list.findIndex((q) => q.id === dragged);
      if (dragged === queue.id || from === -1) return;
      e.preventDefault();
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
      // The default queue stays first, so a queue dropped on it lands just below it.
      landing = from > index && index > 0 ? 'above' : 'below';
      return;
    }
    if (!isDownloadDrag(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    dropping = true;
  }

  /** Leaving the row, not crossing from it into its own button or text, which fires dragleave too. */
  function ondragleave(e: DragEvent) {
    const into = e.relatedTarget;
    if (into instanceof Node && e.currentTarget instanceof Node && e.currentTarget.contains(into))
      return;
    dropping = false;
    landing = null;
  }

  function ondrop(e: DragEvent) {
    const dragged = getDraggedQueue(e);
    if (dragged !== null) {
      e.preventDefault();
      landing = null;
      void app.queues.moveTo(dragged, index);
      return;
    }
    if (!isDownloadDrag(e)) return;
    e.preventDefault();
    e.stopPropagation();
    dropping = false;
    const ids = getDragIds(e);
    if (ids.length > 0) void app.actions.moveToQueue(ids, queue.id);
  }
</script>

<!-- A drop anywhere ends the drag too: the source row may never get its dragend. -->
<svelte:window
  ondrop={() => {
    dragging = false;
    landing = null;
  }}
/>

<div
  role="presentation"
  class={cn(
    'rounded-sm',
    dropping && 'bg-accent-soft outline-2 -outline-offset-2 outline-accent',
    dragging && 'opacity-dragged',
    takes.kind === 'auto' && !takes.enabled && !dragging && 'opacity-quiet',
    // Centred on the edge, so "below one row" and "above the next" draw the same line.
    landing !== null &&
      'relative after:absolute after:inset-x-1 after:h-0.5 after:rounded-full after:bg-accent',
    landing === 'above' && 'after:-top-px',
    landing === 'below' && 'after:-bottom-px',
  )}
  draggable={!queue.is_default}
  ondragstart={(e) => {
    if (queue.is_default) return;
    startQueueDrag(e, queue.id, queue.settings.name);
    dragging = true;
  }}
  ondragend={() => {
    endQueueDrag();
    dragging = false;
    landing = null;
  }}
  {ondragover}
  {ondragleave}
  {ondrop}
>
  <SidebarItem
    label={queue.settings.name}
    icon={ListOrdered}
    active={shown}
    count={speed > 0 ? undefined : count}
    description={doing}
    onclick={() => app.show({ kind: 'queue', id: queue.id })}
  >
    {#snippet trailing()}
      {#if proxied}
        <span class="mr-1.5 flex text-ink-muted" {@attach tooltip(() => PROXY_NOTE)}>
          <Icon icon={Network} size="xs" />
          <span class="sr-only">{PROXY_NOTE}</span>
        </span>
      {/if}
      {#if speed > 0}
        <span class="text-small text-accent tabular-nums">{formatSpeed(speed)}</span>
      {:else if count > 0}
        <span class="text-small text-ink-muted tabular-nums">{count}</span>
      {/if}
    {/snippet}
    {#snippet actions()}
      <Menu {entries} label="{queue.settings.name} queue" align="start">
        {#snippet trigger(props: MenuTriggerProps)}
          <IconButton
            {...props}
            icon={Ellipsis}
            label="Actions for {queue.settings.name}"
            size="xs"
          />
        {/snippet}
      </Menu>
    {/snippet}
  </SidebarItem>
</div>

{#snippet doing()}
  <span class="flex min-w-0 items-center gap-1">
    {#if takes.kind === 'auto'}
      <Badge size="sm">{takes.enabled ? 'Auto' : 'Auto off'}</Badge>
    {/if}
    <span class="truncate" {@attach tooltip(() => textIfCut(status))}>{status}</span>
  </span>
{/snippet}
