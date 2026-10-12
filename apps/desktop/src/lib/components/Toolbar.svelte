<script lang="ts">
  import PanelRight from '@lucide/svelte/icons/panel-right';
  import Pause from '@lucide/svelte/icons/pause';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Play from '@lucide/svelte/icons/play';
  import Plus from '@lucide/svelte/icons/plus';
  import Search from '@lucide/svelte/icons/search';
  import { CATEGORY_LABEL } from '$lib/format';
  import { ariaShortcut } from '$lib/keymap';
  import { getApp } from '$lib/state/app.svelte';
  import { SORT_LABEL, STATUS_GROUP_LABEL, type SortKey } from '$lib/state/view';
  import Button from './Button.svelte';
  import Icon from './Icon.svelte';

  const app = getApp();

  const queue = $derived(app.view.kind === 'queue' ? app.queues.byId(app.view.id) : undefined);

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
  <div class="row">
    <div class="title">
      <h1>{title}</h1>
      <p class="meta num">
        {count === 1 ? '1 download' : `${count.toLocaleString('en-US')} downloads`}
        {#if app.search.trim()}
          matching "{app.search.trim()}"
        {/if}
      </p>
    </div>

    <div class="tools">
      <Button
        icon={Pause}
        onclick={() => app.pauseAll()}
        disabled={!canPauseAll}
        title={canPauseAll ? 'Pause every download' : 'Nothing is running or waiting'}
        ><span class="label">Pause all</span>
      </Button>
      <Button
        icon={Play}
        onclick={() => app.resumeAll()}
        disabled={!canResumeAll}
        title={canResumeAll ? 'Resume every paused download' : 'Nothing is paused'}
        ><span class="label">Resume all</span>
      </Button>
      {#if app.wide}
        <Button
          icon={PanelRight}
          aria-pressed={app.detailsOpen}
          title={app.detailsOpen ? 'Hide the details panel' : 'Show the details panel'}
          onclick={() => (app.detailsOpen = !app.detailsOpen)}
          ><span class="label">Details</span>
        </Button>
      {/if}
      <Button
        variant="primary"
        icon={Plus}
        aria-keyshortcuts={ariaShortcut('add')}
        onclick={() => app.openDialog({ kind: 'add', urls: [] })}
      >
        Add download
      </Button>
    </div>
  </div>

  <div class="row">
    {#if queue}
      <div class="queue-actions" role="group" aria-label="This queue">
        <Button
          variant="ghost"
          small
          icon={Pencil}
          onclick={() => app.openDialog({ kind: 'queue', id: queue.id })}
        >
          Edit queue
        </Button>
        {#if queue.paused}
          <Button
            variant="ghost"
            small
            icon={Play}
            onclick={() => app.queues.setPaused(queue.id, false)}
          >
            Resume queue
          </Button>
        {:else}
          <Button
            variant="ghost"
            small
            icon={Pause}
            onclick={() => app.queues.setPaused(queue.id, true)}
          >
            Pause queue
          </Button>
        {/if}
        {#if app.queues.isOutsideSchedule(queue)}
          <Button variant="ghost" small icon={Play} onclick={() => app.queues.startNow(queue.id)}>
            Start now
          </Button>
        {/if}
      </div>
    {/if}

    <div class="filters">
      <div class="search">
        <Icon icon={Search} size="sm" />
        <input
          id="list-search"
          class="field"
          type="search"
          placeholder="Search names and links"
          aria-label="Search downloads"
          aria-keyshortcuts={ariaShortcut('search')}
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
    </div>
  </div>
</header>

<style>
  .toolbar {
    display: grid;
    gap: 10px;
    padding: 16px 20px 10px;
    border-bottom: 1px solid var(--line);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-width: 0;
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

  /* The queue's own actions share the second line with search and sort, always on one line. */
  .queue-actions {
    display: flex;
    flex: none;
    flex-wrap: nowrap;
    gap: 2px;
    margin-left: -9px;
    white-space: nowrap;
  }

  /* One line each: these keep their size and the title gives way. */
  .tools,
  .filters {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
  }

  .filters {
    margin-left: auto;
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

  /* Labels give way to icons only when the room is really short (the smallest window still has
     them); the names stay for screen readers and tooltips. */
  @container main (max-width: 640px) {
    .search input {
      width: 150px;
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

    .tools :global(.btn):has(.label) {
      padding: 0 8px;
    }
  }
</style>
