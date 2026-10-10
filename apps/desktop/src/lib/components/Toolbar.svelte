<script lang="ts">
  import PanelRight from '@lucide/svelte/icons/panel-right';
  import Pause from '@lucide/svelte/icons/pause';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Play from '@lucide/svelte/icons/play';
  import Plus from '@lucide/svelte/icons/plus';
  import Search from '@lucide/svelte/icons/search';
  import { CATEGORY_LABEL } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import { SORT_LABEL, STATUS_GROUP_LABEL, type SortKey } from '$lib/state/view';

  const app = getApp();

  const queue = $derived(app.view.kind === 'queue' ? app.queueById(app.view.id) : undefined);

  const title = $derived.by(() => {
    const v = app.view;
    switch (v.kind) {
      case 'all':
        return 'All downloads';
      case 'status':
        return STATUS_GROUP_LABEL[v.group];
      case 'queue':
        return queue?.name ?? 'Queue';
      case 'category':
        return CATEGORY_LABEL[v.category];
    }
  });

  const sorts = Object.keys(SORT_LABEL) as SortKey[];
  const count = $derived(app.visible.length);
  // Nothing to pause or resume means the button would do nothing; say so by disabling it.
  const canPauseAll = $derived(app.counts.status.active + app.counts.status.waiting > 0);
  const canResumeAll = $derived(app.counts.status.paused > 0);
</script>

<header class="toolbar">
  <div class="title">
    <h1>{title}</h1>
    <p class="meta num">
      {count === 1 ? '1 download' : `${count.toLocaleString('en-US')} downloads`}
      {#if app.search.trim()}
        matching "{app.search.trim()}"
      {/if}
    </p>
    {#if queue}
      <div class="queue-actions">
        <button
          class="btn ghost small"
          onclick={() => app.openDialog({ kind: 'queue', id: queue.id })}
        >
          <Pencil size={13} aria-hidden="true" /> Edit queue
        </button>
        {#if queue.paused}
          <button class="btn ghost small" onclick={() => app.setQueuePaused(queue.id, false)}>
            <Play size={13} aria-hidden="true" /> Resume queue
          </button>
        {:else}
          <button class="btn ghost small" onclick={() => app.setQueuePaused(queue.id, true)}>
            <Pause size={13} aria-hidden="true" /> Pause queue
          </button>
        {/if}
        {#if app.isOutsideSchedule(queue)}
          <button class="btn ghost small" onclick={() => app.startQueueNow(queue.id)}>
            <Play size={13} aria-hidden="true" /> Start now
          </button>
        {/if}
      </div>
    {/if}
  </div>

  <div class="tools">
    <div class="search">
      <Search size={14} aria-hidden="true" />
      <input
        id="list-search"
        class="field"
        type="search"
        placeholder="Search names and links"
        aria-label="Search downloads"
        aria-keyshortcuts="Control+F"
        bind:value={app.search}
        onkeydown={(e) => {
          if (e.key === 'Escape' && app.search) {
            e.stopPropagation();
            app.search = '';
          } else if (e.key === 'ArrowDown') {
            e.preventDefault();
            document.getElementById('download-list')?.focus();
          }
        }}
      />
    </div>
    <label class="sort">
      <span class="sort-label">Sort</span>
      <select class="field" aria-label="Sort" bind:value={app.sort}>
        {#each sorts as key (key)}
          <option value={key}>{SORT_LABEL[key]}</option>
        {/each}
      </select>
    </label>
    <button
      class="btn"
      onclick={() => app.pauseAll()}
      disabled={!canPauseAll}
      title={canPauseAll ? 'Pause every download' : 'Nothing is running or waiting'}
    >
      <Pause size={14} aria-hidden="true" /><span class="label">Pause all</span>
    </button>
    <button
      class="btn"
      onclick={() => app.resumeAll()}
      disabled={!canResumeAll}
      title={canResumeAll ? 'Resume every paused download' : 'Nothing is paused'}
    >
      <Play size={14} aria-hidden="true" /><span class="label">Resume all</span>
    </button>
    {#if app.wide}
      <button
        class="btn"
        aria-pressed={app.detailsOpen}
        title={app.detailsOpen ? 'Hide the details panel' : 'Show the details panel'}
        onclick={() => (app.detailsOpen = !app.detailsOpen)}
      >
        <PanelRight size={14} aria-hidden="true" /><span class="label">Details</span>
      </button>
    {/if}
    <button
      class="btn primary"
      aria-keyshortcuts="Control+N"
      onclick={() => app.openDialog({ kind: 'add', urls: [] })}
    >
      <Plus size={15} aria-hidden="true" /> Add download
    </button>
  </div>
</header>

<style>
  .toolbar {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 16px;
    padding: 18px 20px 12px;
    border-bottom: 1px solid var(--line);
  }

  .title {
    flex: 1;
    min-width: 0;
  }

  h1 {
    margin: 0;
    overflow: hidden;
    font-size: var(--text-xl);
    font-weight: 620;
    letter-spacing: -0.01em;
    line-height: 1.15;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    margin: 2px 0 0;
    overflow: hidden;
    color: var(--ink-2);
    font-size: var(--text-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .queue-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    margin: 6px 0 0 -9px;
  }

  /* One line, always: the tools keep their size and the title gives way. */
  .tools {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
  }

  .search {
    position: relative;
    display: flex;
    align-items: center;
    color: var(--ink-3);
  }

  .search :global(svg) {
    position: absolute;
    left: 9px;
    pointer-events: none;
  }

  .search input {
    width: 210px;
    padding-left: 29px;
  }

  .sort {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  /* With less room, labels give way to icons; the names stay for screen readers and tooltips. */
  @container main (max-width: 999px) {
    .search input {
      width: 160px;
    }

    .sort-label,
    .label {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip: rect(0 0 0 0);
      white-space: nowrap;
    }

    .btn:has(.label) {
      padding: 0 8px;
    }
  }
</style>
