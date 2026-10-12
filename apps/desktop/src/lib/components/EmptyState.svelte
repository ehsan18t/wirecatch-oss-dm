<script lang="ts">
  import Plus from '@lucide/svelte/icons/plus';
  import { PRODUCT_NAME } from '@wirecatch/product';
  import { CATEGORY_LABEL } from '$lib/format';
  import { shortcutParts } from '$lib/keymap';
  import { getApp } from '$lib/state/app.svelte';
  import { STATUS_GROUP_LABEL } from '$lib/state/view';
  import Button from './Button.svelte';

  const app = getApp();
  const pasteKeys = shortcutParts('paste');

  const firstRun = $derived(app.downloads.size === 0 && app.counts.all === 0);
  const searching = $derived(app.search.trim() !== '');
  const queue = $derived(app.view.kind === 'queue' ? app.queues.byId(app.view.id) : undefined);

  const otherQueue = $derived(
    app.queues.list.find((q) => q.id !== queue?.id)?.name ?? 'another queue',
  );
</script>

<div class="empty" role="status">
  {#if firstRun}
    <div class="first">
      <svg class="art" viewBox="0 0 240 64" aria-hidden="true">
        <path d="M4 14h82c18 0 22 18 40 18h110" />
        <path d="M4 32h236" />
        <path d="M4 50h82c18 0 22-18 40-18h110" />
      </svg>
      <h2>
        Copy a link, then press {#each pasteKeys as key, i (i)}{i > 0 ? ' ' : ''}<kbd>{key}</kbd
          >{/each} here
      </h2>
      <p>
        {PRODUCT_NAME} downloads it right away, using several connections at once. Downloads you start
        in your browser show up here too once the {PRODUCT_NAME} extension is installed.
      </p>
      <div class="actions">
        <Button
          variant="primary"
          icon={Plus}
          onclick={() => app.openDialog({ kind: 'add', urls: [] })}
        >
          Add download
        </Button>
        <Button onclick={() => (app.paletteOpen = true)}
          >See everything {PRODUCT_NAME} can do</Button
        >
      </div>
    </div>
  {:else if searching}
    <h2>Nothing matches "{app.search.trim()}"</h2>
    <p>The search looks at file names and links in this view.</p>
    <div class="actions">
      <Button onclick={() => (app.search = '')}>Clear search</Button>
      {#if app.view.kind !== 'all'}
        <Button variant="ghost" onclick={() => app.show({ kind: 'all' })}
          >Search all downloads</Button
        >
      {/if}
    </div>
  {:else if queue}
    <h2>{queue.name} is empty</h2>
    <p>
      Add a download straight into this queue, or drag downloads onto it in the sidebar from
      {otherQueue} or any other list.
    </p>
    <div class="actions">
      <Button
        variant="primary"
        icon={Plus}
        onclick={() => app.openDialog({ kind: 'add', urls: [], queue: queue.id })}
      >
        Add to {queue.name}
      </Button>
    </div>
  {:else}
    {@const v = app.view}
    <h2>
      {#if v.kind === 'status'}
        {v.group === 'failed'
          ? 'Nothing needs your attention'
          : `Nothing in ${STATUS_GROUP_LABEL[v.group].toLowerCase()}`}
      {:else if v.kind === 'category'}
        No {CATEGORY_LABEL[v.category].toLowerCase()} yet
      {:else}
        No downloads here
      {/if}
    </h2>
    <p>Downloads appear here as soon as they match this view.</p>
    <div class="actions">
      <Button onclick={() => app.show({ kind: 'all' })}>Show all downloads</Button>
    </div>
  {/if}
</div>

<style>
  .empty {
    max-width: 460px;
    margin: 72px auto 0;
    padding: 0 24px;
    color: var(--ink-2);
  }

  h2 {
    margin: 0 0 8px;
    color: var(--ink);
    font-size: var(--text-lg);
    font-weight: 620;
  }

  .first h2 {
    font-size: var(--text-xl);
    letter-spacing: -0.01em;
  }

  h2 kbd {
    font-size: var(--text-sm);
    line-height: 20px;
    vertical-align: 2px;
  }

  p {
    margin: 0;
    line-height: 1.55;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 18px;
  }

  .art {
    display: block;
    width: 240px;
    margin-bottom: 22px;
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
    stroke-linecap: round;
  }

  .art path:nth-child(1),
  .art path:nth-child(3) {
    stroke: var(--wire-done);
  }
</style>
