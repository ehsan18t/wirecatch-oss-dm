<script lang="ts">
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
        <button class="btn ghost small" onclick={() => app.setQueueRunning(queue.id, false)}>
          <Pause size={13} aria-hidden="true" /> Pause queue
        </button>
        <button class="btn ghost small" onclick={() => app.setQueueRunning(queue.id, true)}>
          <Play size={13} aria-hidden="true" /> Resume queue
        </button>
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
      <span>Sort</span>
      <select class="field" bind:value={app.sort}>
        {#each sorts as key (key)}
          <option value={key}>{SORT_LABEL[key]}</option>
        {/each}
      </select>
    </label>
    <div class="group" role="group" aria-label="Everything">
      <button class="btn" onclick={() => app.pauseAll()} title="Pause every download">
        <Pause size={14} aria-hidden="true" /> Pause all
      </button>
      <button class="btn" onclick={() => app.resumeAll()} title="Resume every paused download">
        <Play size={14} aria-hidden="true" /> Resume all
      </button>
    </div>
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
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    gap: 12px 20px;
    padding: 18px 20px 12px;
    border-bottom: 1px solid var(--line);
  }

  .title {
    min-width: 0;
  }

  h1 {
    margin: 0;
    font-size: var(--text-xl);
    font-weight: 620;
    letter-spacing: -0.01em;
    line-height: 1.15;
  }

  .meta {
    margin: 2px 0 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .queue-actions {
    display: flex;
    gap: 2px;
    margin: 6px 0 0 -9px;
  }

  .tools {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
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
    width: 220px;
    padding-left: 29px;
  }

  .sort {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .group {
    display: flex;
    gap: 4px;
  }
</style>
