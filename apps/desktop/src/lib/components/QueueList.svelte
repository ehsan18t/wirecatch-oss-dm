<script lang="ts">
  // The queues in the sidebar: what each is doing, its schedule and limit, and its own buttons.
  // Downloads dragged onto a queue move to it.
  import Clock from '@lucide/svelte/icons/clock';
  import Gauge from '@lucide/svelte/icons/gauge';
  import Pause from '@lucide/svelte/icons/pause';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Play from '@lucide/svelte/icons/play';
  import type { Queue } from '@wirecatch/protocol';
  import { formatRate, formatSpeed } from '@wirecatch/units';
  import { getDragIds, isDownloadDrag } from '$lib/dnd';
  import { formatSchedule } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import Button from './Button.svelte';
  import Icon from './Icon.svelte';
  import IconButton from './IconButton.svelte';

  const app = getApp();

  let dropTarget = $state<number | null>(null);

  function shown(q: Queue): boolean {
    return app.screen === 'downloads' && app.view.kind === 'queue' && app.view.id === q.id;
  }

  function ondragover(e: DragEvent, q: Queue) {
    if (!isDownloadDrag(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    dropTarget = q.id;
  }

  function ondrop(e: DragEvent, q: Queue) {
    e.preventDefault();
    dropTarget = null;
    const ids = getDragIds(e);
    if (ids.length > 0) void app.moveToQueue(ids, q.id);
  }
</script>

<ul class="queues">
  {#each app.queues.list as q (q.id)}
    {@const qspeed = app.downloads.queueSpeed[q.id] ?? 0}
    {@const c = app.counts.queue.get(q.id)}
    {@const status = app.queues.status(q)}
    <li
      class="queue"
      class:paused={q.paused}
      class:drop={dropTarget === q.id}
      ondragover={(e) => ondragover(e, q)}
      ondragleave={() => (dropTarget = null)}
      ondrop={(e) => ondrop(e, q)}
    >
      <button
        type="button"
        class="queue-main"
        aria-current={shown(q) ? 'page' : undefined}
        onclick={() => app.show({ kind: 'queue', id: q.id })}
      >
        <span class="qline">
          <span class="qname">{q.name}</span>
          {#if qspeed > 0}
            <span class="qspeed num">{formatSpeed(qspeed)}</span>
          {:else if c && c.total > 0}
            <span class="count num">{c.total}</span>
          {/if}
        </span>
        <span class="qstate num">
          {#if q.paused}<Icon icon={Pause} size="xs" />{/if}
          {status}{status === 'Idle' ? `, up to ${q.max_concurrent} at once` : ''}
        </span>
        <span class="qmeta">
          <span
            ><Icon icon={Clock} size="xs" />
            {q.schedule ? formatSchedule(q.schedule) : 'Any time'}</span
          >
          <span
            ><Icon icon={Gauge} size="xs" />
            {q.speed_limit === null ? 'No limit' : formatRate(q.speed_limit)}</span
          >
        </span>
      </button>
      <IconButton
        class="edit"
        icon={Pencil}
        label="Edit {q.name}: name, schedule and limits"
        title="Edit name, schedule and limits"
        onclick={() => app.openDialog({ kind: 'queue', id: q.id })}
      />
      <IconButton
        class="toggle"
        icon={q.paused ? Play : Pause}
        label={q.paused ? `Resume the ${q.name} queue` : `Pause the ${q.name} queue`}
        title={q.paused ? 'Resume this queue' : 'Pause this queue'}
        onclick={() => app.queues.setPaused(q.id, !q.paused)}
      />
      {#if app.queues.isOutsideSchedule(q) && !q.paused}
        <Button small class="start-now" icon={Play} onclick={() => app.queues.startNow(q.id)}>
          Start now
        </Button>
      {/if}
    </li>
  {/each}
</ul>

<style>
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .queue {
    position: relative;
    border-radius: var(--radius-m);
  }

  .queue.drop {
    outline: 2px dashed var(--accent);
    outline-offset: -2px;
    background: var(--accent-soft);
  }

  .queue-main {
    display: flex;
    flex-direction: column;
    gap: 1px;
    width: 100%;
    padding: 7px 36px 8px 10px;
    border: 0;
    border-radius: var(--radius-m);
    background: transparent;
    color: var(--ink-2);
    text-align: left;
    cursor: pointer;
  }

  .queue-main:hover {
    background: var(--surface-3);
  }

  .queue-main[aria-current='page'] {
    background: var(--surface);
    color: var(--ink);
    box-shadow: inset 3px 0 0 var(--accent);
  }

  .qline {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .qname {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--ink);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .qspeed {
    color: var(--accent-text);
    font-size: var(--text-sm);
    font-weight: 600;
  }

  /* The same pill the sidebar's other counts have. */
  .count {
    min-width: 20px;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: var(--text-xs);
    line-height: 18px;
    text-align: center;
  }

  .qstate {
    font-size: var(--text-sm);
  }

  .qmeta {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 10px;
    margin-top: 2px;
    color: var(--ink-3);
    font-size: var(--text-xs);
  }

  .qmeta span {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .queue > :global(.edit),
  .queue > :global(.toggle) {
    position: absolute;
    right: 4px;
    color: var(--ink-2);
  }

  .queue > :global(.edit) {
    top: 5px;
  }

  .queue > :global(.toggle) {
    top: 33px;
  }

  .queue.paused .qstate {
    color: var(--warn);
    font-weight: 600;
  }

  .qstate :global(svg) {
    vertical-align: -1px;
  }

  .queue > :global(.start-now) {
    margin: 0 0 6px 10px;
  }
</style>
