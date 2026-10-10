<script lang="ts">
  import Clock from '@lucide/svelte/icons/clock';
  import Command from '@lucide/svelte/icons/command';
  import Gauge from '@lucide/svelte/icons/gauge';
  import Keyboard from '@lucide/svelte/icons/keyboard';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Plus from '@lucide/svelte/icons/plus';
  import SettingsIcon from '@lucide/svelte/icons/settings';
  import type { Queue } from '@wirecatch/protocol';
  import { getDragIds, isDownloadDrag } from '$lib/dnd';
  import { CATEGORIES, CATEGORY_LABEL, formatRate, formatSchedule, formatSpeed } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import { sameView, STATUS_GROUP_LABEL, STATUS_GROUPS, type View } from '$lib/state/view';
  import CategoryIcon from './CategoryIcon.svelte';

  const app = getApp();

  let dropTarget = $state<number | null>(null);

  const speed = $derived(formatSpeed(app.downloads.totalSpeed));
  const active = $derived(app.counts.status.active);
  const waiting = $derived(app.counts.status.waiting);

  function current(view: View): boolean {
    return app.screen === 'downloads' && sameView(app.view, view);
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

<nav class="sidebar" aria-label="Views and queues">
  <header class="readout" aria-live="off">
    <p class="brand">
      <svg viewBox="0 0 20 12" width="20" height="12" aria-hidden="true">
        <path
          d="M1 6h6l2-4 2 8 2-4h6"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
      Wirecatch
    </p>
    <p class="speed num" title="Total download speed">{speed || 'Idle'}</p>
    <p class="sub num">
      {#if active + waiting === 0}
        Nothing running
      {:else}
        {active} downloading{waiting > 0 ? `, ${waiting} waiting` : ''}
      {/if}
    </p>
  </header>

  <button class="palette" onclick={() => (app.paletteOpen = true)}>
    <Command size={14} aria-hidden="true" />
    <span>Find any action</span>
    <kbd>Ctrl K</kbd>
  </button>

  <div class="scroll">
    <section aria-labelledby="h-downloads">
      <h2 id="h-downloads" class="visually-hidden">Downloads by status</h2>
      <ul>
        <li>
          <button
            class="item"
            aria-current={current({ kind: 'all' }) ? 'page' : undefined}
            onclick={() => app.show({ kind: 'all' })}
          >
            <span class="label">All downloads</span>
            <span class="count num">{app.counts.all}</span>
          </button>
        </li>
        {#each STATUS_GROUPS as group (group)}
          {@const n = app.counts.status[group]}
          <li>
            <button
              class="item"
              class:alert={group === 'failed' && n > 0}
              aria-current={current({ kind: 'status', group }) ? 'page' : undefined}
              onclick={() => app.show({ kind: 'status', group })}
            >
              <span class="dot {group}" aria-hidden="true"></span>
              <span class="label">{STATUS_GROUP_LABEL[group]}</span>
              {#if n > 0}<span class="count num">{n}</span>{/if}
            </button>
          </li>
        {/each}
      </ul>
    </section>

    <section aria-labelledby="h-queues">
      <div class="section-head">
        <h2 id="h-queues">Queues</h2>
        <button class="btn ghost small" onclick={() => app.openDialog({ kind: 'queue', id: null })}>
          <Plus size={14} aria-hidden="true" /> New queue
        </button>
      </div>
      <ul class="queues">
        {#each app.queues as q (q.id)}
          {@const qspeed = app.downloads.queueSpeed[q.id] ?? 0}
          {@const c = app.counts.queue.get(q.id)}
          {@const status = app.queueStatus(q)}
          <li
            class="queue"
            class:drop={dropTarget === q.id}
            ondragover={(e) => ondragover(e, q)}
            ondragleave={() => (dropTarget = null)}
            ondrop={(e) => ondrop(e, q)}
          >
            <button
              class="queue-main"
              aria-current={current({ kind: 'queue', id: q.id }) ? 'page' : undefined}
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
              <span class="qstate num"
                >{status}{status === 'Idle' ? `, up to ${q.max_concurrent} at once` : ''}</span
              >
              <span class="qmeta">
                <span
                  ><Clock size={12} aria-hidden="true" />
                  {q.schedule ? formatSchedule(q.schedule) : 'Any time'}</span
                >
                <span
                  ><Gauge size={12} aria-hidden="true" />
                  {q.speed_limit === null ? 'No limit' : formatRate(q.speed_limit)}</span
                >
              </span>
            </button>
            <button
              class="icon-btn edit"
              aria-label="Edit {q.name}: name, schedule and limits"
              title="Edit name, schedule and limits"
              onclick={() => app.openDialog({ kind: 'queue', id: q.id })}
            >
              <Pencil size={14} aria-hidden="true" />
            </button>
          </li>
        {/each}
      </ul>
    </section>

    <section aria-labelledby="h-types">
      <div class="section-head"><h2 id="h-types">File types</h2></div>
      <ul>
        {#each CATEGORIES as category (category)}
          {@const n = app.counts.category[category]}
          <li>
            <button
              class="item"
              class:empty={n === 0}
              aria-current={current({ kind: 'category', category }) ? 'page' : undefined}
              onclick={() => app.show({ kind: 'category', category })}
            >
              <CategoryIcon {category} size={14} />
              <span class="label">{CATEGORY_LABEL[category]}</span>
              {#if n > 0}<span class="count num">{n}</span>{/if}
            </button>
          </li>
        {/each}
      </ul>
    </section>
  </div>

  <footer class="foot">
    <button
      class="item"
      aria-current={app.screen === 'settings' ? 'page' : undefined}
      onclick={() => app.openSettings()}
    >
      <SettingsIcon size={15} aria-hidden="true" />
      <span class="label">Settings</span>
      <kbd>Ctrl ,</kbd>
    </button>
    <button class="item" onclick={() => app.openDialog({ kind: 'shortcuts' })}>
      <Keyboard size={15} aria-hidden="true" />
      <span class="label">Keyboard shortcuts</span>
      <kbd>F1</kbd>
    </button>
    {#if app.backend.kind === 'mock'}
      <p class="sample">Sample data. Nothing is really downloading.</p>
    {/if}
  </footer>
</nav>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--bg);
    color: var(--ink-2);
  }

  p {
    margin: 0;
  }

  .readout {
    padding: 16px 16px 12px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--ink);
    font-size: var(--text-sm);
    font-weight: 650;
    letter-spacing: 0.01em;
  }

  .brand svg {
    color: var(--accent);
  }

  /* The one loud thing in the chrome: how fast everything is moving right now. */
  .speed {
    margin-top: 12px;
    color: var(--ink);
    font-size: var(--text-readout);
    font-weight: 600;
    font-variation-settings: 'wdth' 78;
    line-height: 1.05;
    letter-spacing: -0.01em;
  }

  .sub {
    margin-top: 2px;
    font-size: var(--text-sm);
  }

  .palette {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 10px 8px;
    height: 32px;
    padding: 0 8px 0 10px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-m);
    background: var(--surface);
    color: var(--ink-2);
    font-size: var(--text-sm);
    cursor: pointer;
    text-align: left;
  }

  .palette span {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .palette:hover {
    color: var(--ink);
  }

  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 4px 8px 12px;
  }

  section + section {
    margin-top: 14px;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 0 4px 8px;
  }

  h2 {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
    font-weight: 650;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 30px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-m);
    background: transparent;
    color: var(--ink-2);
    font-size: var(--text-md);
    text-align: left;
    cursor: pointer;
  }

  .item:hover {
    background: var(--surface-3);
    color: var(--ink);
  }

  .item[aria-current='page'],
  .queue-main[aria-current='page'] {
    background: var(--surface);
    color: var(--ink);
    box-shadow: inset 3px 0 0 var(--accent);
  }

  .item.empty {
    color: var(--ink-3);
  }

  .item.alert .label,
  .item.alert .count {
    color: var(--danger);
    font-weight: 600;
  }

  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    color: var(--ink-3);
    font-size: var(--text-sm);
  }

  .dot {
    width: 8px;
    height: 8px;
    margin: 0 3px;
    border-radius: 50%;
    background: var(--wire-done);
  }

  .dot.active {
    background: var(--accent);
  }

  .dot.paused {
    background: var(--warn-wire);
  }

  .dot.failed {
    background: var(--danger);
  }

  .dot.waiting {
    background: transparent;
    border: 1.5px solid var(--ink-3);
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

  .edit {
    position: absolute;
    top: 5px;
    right: 4px;
    color: var(--ink-3);
  }

  .foot {
    padding: 8px;
    border-top: 1px solid var(--line);
  }

  .sample {
    margin: 6px 8px 2px;
    color: var(--ink-3);
    font-size: var(--text-xs);
  }
</style>
