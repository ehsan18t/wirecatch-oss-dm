<script lang="ts">
  import Clock from '@lucide/svelte/icons/clock';
  import Command from '@lucide/svelte/icons/command';
  import Gauge from '@lucide/svelte/icons/gauge';
  import Keyboard from '@lucide/svelte/icons/keyboard';
  import Bell from '@lucide/svelte/icons/bell';
  import Pause from '@lucide/svelte/icons/pause';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Play from '@lucide/svelte/icons/play';
  import Plus from '@lucide/svelte/icons/plus';
  import SettingsIcon from '@lucide/svelte/icons/settings';
  import type { Queue } from '@wirecatch/protocol';
  import { getDragIds, isDownloadDrag } from '$lib/dnd';
  import { CATEGORIES, CATEGORY_LABEL, formatSchedule } from '$lib/format';
  import { shortcutParts } from '$lib/keymap';
  import { formatRate, formatSpeed } from '@wirecatch/units';
  import { getApp } from '$lib/state/app.svelte';
  import { sameView, STATUS_GROUP_LABEL, STATUS_GROUPS, type View } from '$lib/state/view';
  import Button from './Button.svelte';
  import CategoryIcon from './CategoryIcon.svelte';
  import Icon from './Icon.svelte';
  import IconButton from './IconButton.svelte';

  const app = getApp();

  let dropTarget = $state<number | null>(null);

  const speed = $derived(formatSpeed(app.downloads.totalSpeed));
  const active = $derived(app.counts.status.active);
  const waiting = $derived(app.counts.status.waiting);
  const joining = $derived(app.counts.joining);
  /** A phrase in the readout longer than this does not fit the column at the size of a speed. */
  const HEADLINE_FITS = 8;
  /** What the readout says when nothing is moving bytes. */
  const headline = $derived(
    active === 0 ? 'Idle' : joining === active ? 'Finishing videos' : 'Starting',
  );

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
    <p
      class="speed num"
      class:word={!speed && headline.length > HEADLINE_FITS}
      title="Total download speed"
    >
      {speed || headline}
    </p>
    <p class="sub num">
      {#if active + waiting === 0}
        Nothing running
      {:else}
        {[
          active - joining > 0 || joining === 0 ? `${active - joining} downloading` : '',
          joining > 0 ? `${joining} being joined or converted` : '',
          waiting > 0 ? `${waiting} waiting` : '',
        ]
          .filter(Boolean)
          .join(', ')}
      {/if}
    </p>
  </header>

  {#if app.questions.waitingCount > 0}
    <!-- Each waiting download has its own window (D23); this brings them all back. -->
    <button
      type="button"
      class="pending"
      title="Show the windows of the downloads that wait for your answer"
      onclick={() => app.showQuestionWindows()}
    >
      <Icon icon={Bell} size="sm" />
      <span class="waiting-label">Waiting for you</span>
      <span
        class="count num"
        aria-label="{app.questions.waitingCount} {app.questions.waitingCount === 1
          ? 'download'
          : 'downloads'}">{app.questions.waitingCount}</span
      >
    </button>
  {/if}

  <button type="button" class="palette" onclick={() => (app.paletteOpen = true)}>
    <Icon icon={Command} size="sm" />
    <span>Find any action</span>
    <kbd>{shortcutParts('palette').join(' ')}</kbd>
  </button>

  <div class="scroll">
    <section aria-labelledby="h-downloads">
      <h2 id="h-downloads" class="visually-hidden">Downloads by status</h2>
      <ul>
        <li>
          <button
            type="button"
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
          {#if group !== 'cancelled' || n > 0 || current({ kind: 'status', group })}
            <li>
              <button
                type="button"
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
          {/if}
        {/each}
      </ul>
    </section>

    <section aria-labelledby="h-queues">
      <div class="section-head">
        <h2 id="h-queues">Queues</h2>
        <Button
          variant="ghost"
          small
          icon={Plus}
          onclick={() => app.openDialog({ kind: 'queue', id: null })}
        >
          New queue
        </Button>
      </div>
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
    </section>

    <section aria-labelledby="h-types">
      <div class="section-head"><h2 id="h-types">File types</h2></div>
      <ul>
        {#each CATEGORIES as category (category)}
          {@const n = app.counts.category[category]}
          <li>
            <button
              type="button"
              class="item"
              class:empty={n === 0}
              aria-current={current({ kind: 'category', category }) ? 'page' : undefined}
              onclick={() => app.show({ kind: 'category', category })}
            >
              <CategoryIcon {category} size="sm" />
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
      type="button"
      class="item"
      aria-current={app.screen === 'settings' ? 'page' : undefined}
      onclick={() => app.openSettings()}
    >
      <Icon icon={SettingsIcon} size="md" />
      <span class="label">Settings</span>
      <kbd>{shortcutParts('settings').join(' ')}</kbd>
    </button>
    <button type="button" class="item" onclick={() => app.openDialog({ kind: 'shortcuts' })}>
      <Icon icon={Keyboard} size="md" />
      <span class="label">Keyboard shortcuts</span>
      <kbd>{shortcutParts('help').join(' ')}</kbd>
    </button>
    {#if app.sampleData}
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

  /* A phrase instead of a number fits the column at a smaller size. */
  .speed.word {
    font-size: var(--text-lg);
    line-height: 1.3;
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

  .pending {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 10px 8px;
    padding: 7px 10px;
    border: 1px solid var(--accent);
    border-radius: var(--radius-m);
    background: var(--accent-soft);
    color: var(--ink);
    font-size: var(--text-sm);
    font-weight: 600;
    text-align: left;
    cursor: pointer;
  }

  .waiting-label {
    flex: 1;
  }

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

  .pending :global(svg) {
    flex: none;
    color: var(--accent-text);
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
