<script lang="ts">
  import AppWindow from '@lucide/svelte/icons/app-window';
  import Copy from '@lucide/svelte/icons/copy';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import Pause from '@lucide/svelte/icons/pause';
  import Play from '@lucide/svelte/icons/play';
  import RotateCw from '@lucide/svelte/icons/rotate-cw';
  import Trash from '@lucide/svelte/icons/trash-2';
  import X from '@lucide/svelte/icons/x';
  import {
    DEFAULT_CONNECTIONS_PER_DOWNLOAD,
    type FixAction,
    type QueueId,
  } from '@wirecatch/protocol';
  import {
    folderOf,
    formatDate,
    linkForDisplay,
    MEDIA_SOURCE_LABEL,
    shownWait,
    STATUS_LABEL,
    statusDetail,
    statusShort,
  } from '$lib/format';
  import { formatBytes } from '@wirecatch/units';
  import { getApp } from '$lib/state/app.svelte';
  import { isUnderWay } from '$lib/state/view';
  import Button from './Button.svelte';
  import CategoryIcon from './CategoryIcon.svelte';
  import DetailProgress from './DetailProgress.svelte';
  import ErrorFix from './ErrorFix.svelte';
  import IconButton from './IconButton.svelte';

  /** `docked` takes a column; `overlay` floats over the list in narrower windows. */
  let { mode = 'docked' }: { mode?: 'docked' | 'overlay' } = $props();

  const app = getApp();

  const rows = $derived(app.selection.rows);
  const one = $derived(rows.length === 1 ? rows[0] : undefined);

  const anyRunning = $derived(rows.some((r) => isUnderWay(r.status)));
  const anyStopped = $derived(rows.some((r) => r.status === 'paused' || r.status === 'failed'));
  const anyFailed = $derived(rows.some((r) => r.status === 'failed' || r.status === 'cancelled'));
  /** Connections each download may open: the setting, or the core's default before it loads. */
  const connectionsLimit = $derived(
    app.settings?.connections_per_download ?? DEFAULT_CONNECTIONS_PER_DOWNLOAD,
  );
  const totalSize = $derived(rows.reduce((sum, r) => sum + (r.total ?? 0), 0));

  function move(e: Event & { currentTarget: HTMLSelectElement }) {
    const queue: QueueId = Number(e.currentTarget.value);
    void app.actions.moveToQueue(app.selection.list, queue);
  }

  const sharedQueue = $derived(
    rows.length > 0 && rows.every((r) => r.queue === rows[0]?.queue) ? rows[0]?.queue : undefined,
  );
</script>

<aside class="detail {mode}" aria-label="Selected download">
  {#if mode === 'overlay'}
    <IconButton
      class="close"
      icon={X}
      size="md"
      label="Close details and clear the selection"
      title="Close"
      onclick={() => app.selection.clear()}
    />
  {/if}
  {#if one}
    <header>
      <span class="cat"><CategoryIcon category={one.category} size="lg" /></span>
      <h2>{one.file_name}</h2>
      <p class="status {one.status}">
        {one.status === 'processing' ? statusShort(one) : STATUS_LABEL[one.status]}
      </p>
    </header>

    {#if shownWait(one)}
      <p class="waiting">{statusDetail(one, app.queues.byId(one.queue)?.name ?? 'Its queue')}</p>
    {/if}

    {#if one.status === 'failed' && one.error}
      <ErrorFix
        row={one}
        onfix={(fix: FixAction) => app.actions.fix(one.id, fix)}
        onretry={() => app.actions.retry([one.id])}
      />
    {/if}

    {#if one.media}
      <p class="waiting">
        {one.media.quality}, {MEDIA_SOURCE_LABEL[one.media.source]}.
        {#if one.status === 'processing'}
          {statusDetail(one, '')}
        {/if}
      </p>
    {/if}

    <DetailProgress row={one} />

    <div class="actions">
      {#if isUnderWay(one.status)}
        <Button icon={Pause} onclick={() => app.actions.pause([one.id])}>Pause</Button>
      {:else if one.status === 'paused'}
        <Button variant="primary" icon={Play} onclick={() => app.actions.resume([one.id])}
          >Resume</Button
        >
      {:else if one.status === 'cancelled'}
        <Button variant="primary" icon={RotateCw} onclick={() => app.actions.retry([one.id])}>
          Start over</Button
        >
      {:else if one.status === 'failed' && !one.error}
        <!-- With an error, the error box above already offers Try again. -->
        <Button icon={RotateCw} onclick={() => app.actions.retry([one.id])}>Try again</Button>
      {/if}
      {#if one.status === 'completed'}
        <Button variant="primary" onclick={() => app.actions.openFile(one.id)}>Open</Button>
      {/if}
      <Button icon={FolderOpen} onclick={() => app.actions.openFolder(one.id)}
        >Show in folder</Button
      >
      <Button icon={AppWindow} onclick={() => app.showProgressWindow(one.id)}>
        Show progress window</Button
      >
      <Button icon={Trash} onclick={() => app.openDialog({ kind: 'remove', ids: [one.id] })}>
        Remove
      </Button>
    </div>

    <dl>
      <div>
        <dt><label for="detail-queue">Queue</label></dt>
        <dd>
          <select id="detail-queue" class="field" value={one.queue} onchange={move}>
            {#each app.queues.list as q (q.id)}
              <option value={q.id}>{q.name}</option>
            {/each}
          </select>
        </dd>
      </div>
      <div>
        <dt>Connections</dt>
        <dd class="num">
          {#if one.connections > 0}
            {one.connections} open now, up to {connectionsLimit}
          {:else}
            Up to {connectionsLimit}
          {/if}
        </dd>
      </div>
      <div>
        <dt>Saved to</dt>
        <dd class="path">
          <span>{folderOf(one.path)}</span>
          {#if one.status !== 'completed'}
            <Button small onclick={() => app.actions.chooseFolderFor(one.id)}>Change</Button>
          {/if}
        </dd>
      </div>
      <div>
        <dt>Link</dt>
        <dd class="path">
          <span class="url">{linkForDisplay(one.url)}</span>
          <IconButton
            icon={Copy}
            label="Copy link"
            title="Copy link"
            onclick={() => app.actions.copyLink(one.id)}
          />
        </dd>
      </div>
      {#if !['completed', 'failed', 'cancelled', 'processing'].includes(one.status)}
        <div>
          <dt>Link expired?</dt>
          <dd>
            <Button small onclick={() => app.openDialog({ kind: 'refresh', id: one.id })}
              >Give it a fresh link</Button
            >
          </dd>
        </div>
      {/if}
      <div>
        <dt>Added</dt>
        <dd class="num">{formatDate(one.added_at, app.clock.now)}</dd>
      </div>
      {#if one.completed_at !== null}
        <div>
          <dt>Finished</dt>
          <dd class="num">{formatDate(one.completed_at, app.clock.now)}</dd>
        </div>
      {/if}
    </dl>
    {#if one.status !== 'completed' && one.status !== 'cancelled'}
      <Button
        variant="ghost"
        small
        class="cancel"
        onclick={() => app.openDialog({ kind: 'cancel', ids: [one.id] })}
      >
        Cancel and discard the partial file
      </Button>
    {/if}
  {:else if rows.length > 1}
    <header>
      <h2>{rows.length} downloads selected</h2>
      <p class="status num">{formatBytes(totalSize)} in total</p>
    </header>
    <div class="actions">
      {#if anyRunning}
        <Button icon={Pause} onclick={() => app.actions.pause()}>Pause</Button>
      {/if}
      {#if anyStopped}
        <Button icon={Play} onclick={() => app.actions.resume()}>Resume</Button>
      {/if}
      {#if anyFailed}
        <Button icon={RotateCw} onclick={() => app.actions.retry()}>Try again</Button>
      {/if}
      <Button
        icon={Trash}
        onclick={() => app.openDialog({ kind: 'remove', ids: app.selection.list })}
      >
        Remove
      </Button>
    </div>
    <dl>
      <div>
        <dt><label for="detail-queue-many">Move to queue</label></dt>
        <dd>
          <select id="detail-queue-many" class="field" value={sharedQueue ?? ''} onchange={move}>
            {#if sharedQueue === undefined}
              <option value="" disabled>Choose a queue</option>
            {/if}
            {#each app.queues.list as q (q.id)}
              <option value={q.id}>{q.name}</option>
            {/each}
          </select>
        </dd>
      </div>
    </dl>
    <p class="tip">Tip: drag the selection onto a queue in the sidebar to move it there.</p>
  {:else}
    <div class="none">
      <p>Select a download to see where it is saved, its link and what you can do with it.</p>
      <p>Hold <kbd>Ctrl</kbd> or <kbd>Shift</kbd> to select several.</p>
    </div>
  {/if}
</aside>

<style>
  .detail.overlay {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    z-index: 10;
    box-shadow: var(--shadow);
  }

  .detail > :global(.close) {
    position: absolute;
    top: 10px;
    right: 10px;
  }

  .overlay header {
    padding-right: 28px;
  }

  .waiting {
    margin: -6px 0 0;
    padding: 8px 10px;
    border-left: 3px solid var(--wire-done);
    border-radius: var(--radius-s);
    background: var(--surface-2);
    color: var(--ink-2);
    font-size: var(--text-sm);
    line-height: 1.45;
  }

  .detail {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: var(--detail-width);
    min-height: 0;
    padding: 18px 18px 20px;
    overflow-y: auto;
    border-left: 1px solid var(--line);
    background: var(--surface);
  }

  header {
    display: grid;
    grid-template-columns: auto 1fr;
    column-gap: 10px;
    align-items: start;
  }

  .cat {
    grid-row: span 2;
    padding-top: 2px;
    color: var(--ink-2);
  }

  h2 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 620;
    line-height: 1.3;
    overflow-wrap: anywhere;
  }

  .status {
    margin: 3px 0 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .status.downloading,
  .status.connecting {
    color: var(--accent-text);
  }

  .status.paused {
    color: var(--warn);
  }

  .status.failed {
    color: var(--danger);
    font-weight: 600;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  dl {
    display: grid;
    gap: 10px;
    margin: 0;
    padding-top: 14px;
    border-top: 1px solid var(--line);
  }

  dl > div {
    display: grid;
    gap: 3px;
  }

  dt {
    color: var(--ink-2);
    font-size: var(--text-xs);
    font-weight: 600;
  }

  dd {
    margin: 0;
    font-size: var(--text-sm);
  }

  dd select {
    width: 100%;
  }

  .path {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }

  .path span {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .url {
    color: var(--ink-2);
  }

  .detail > :global(.cancel) {
    align-self: flex-start;
    margin-left: -9px;
    color: var(--ink-2);
  }

  .tip,
  .none p {
    margin: 0 0 8px;
    color: var(--ink-2);
    font-size: var(--text-sm);
    line-height: 1.5;
  }

  .none {
    margin-top: 40px;
  }
</style>
