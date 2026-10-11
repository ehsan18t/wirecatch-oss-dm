<script lang="ts">
  import AppWindow from '@lucide/svelte/icons/app-window';
  import Copy from '@lucide/svelte/icons/copy';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import Pause from '@lucide/svelte/icons/pause';
  import Play from '@lucide/svelte/icons/play';
  import RotateCw from '@lucide/svelte/icons/rotate-cw';
  import Trash from '@lucide/svelte/icons/trash-2';
  import X from '@lucide/svelte/icons/x';
  import type { DownloadSummary, QueueId } from '@wirecatch/protocol';
  import {
    formatBytes,
    formatDate,
    formatEta,
    formatPercent,
    formatSize,
    formatSpeed,
    folderOf,
    linkForDisplay,
    MEDIA_SOURCE_LABEL,
    progressFraction,
    STATUS_LABEL,
    waitLong,
  } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import CategoryIcon from './CategoryIcon.svelte';
  import ErrorFix from './ErrorFix.svelte';

  /** `docked` takes a column; `overlay` floats over the list in narrower windows. */
  let { mode = 'docked' }: { mode?: 'docked' | 'overlay' } = $props();

  const app = getApp();

  const rows = $derived(app.selectedRows);
  const one = $derived(rows.length === 1 ? rows[0] : undefined);

  function isRunning(r: DownloadSummary) {
    return (
      r.status === 'downloading' ||
      r.status === 'connecting' ||
      r.status === 'queued' ||
      r.status === 'processing'
    );
  }

  const anyRunning = $derived(rows.some(isRunning));
  const anyStopped = $derived(rows.some((r) => r.status === 'paused' || r.status === 'failed'));
  const anyFailed = $derived(rows.some((r) => r.status === 'failed' || r.status === 'cancelled'));
  const totalSize = $derived(rows.reduce((sum, r) => sum + (r.total ?? 0), 0));

  async function copyLink(url: string) {
    try {
      await navigator.clipboard.writeText(url);
      app.notify('Link copied.');
    } catch {
      app.fail('Could not copy the link. Select it and copy it by hand.');
    }
  }

  function move(e: Event & { currentTarget: HTMLSelectElement }) {
    const queue = Number(e.currentTarget.value) as QueueId;
    void app.moveToQueue(app.selectedIds, queue);
  }

  const sharedQueue = $derived(
    rows.length > 0 && rows.every((r) => r.queue === rows[0]?.queue) ? rows[0]?.queue : undefined,
  );
</script>

<aside class="detail {mode}" aria-label="Selected download">
  {#if mode === 'overlay'}
    <button
      class="icon-btn close"
      aria-label="Close details and clear the selection"
      title="Close"
      onclick={() => app.clearSelection()}
    >
      <X size={16} aria-hidden="true" />
    </button>
  {/if}
  {#if one}
    {@const fraction = progressFraction(one.done, one.total)}
    <header>
      <span class="cat"><CategoryIcon category={one.category} size={18} /></span>
      <h2>{one.file_name}</h2>
      <p class="status {one.status}">{STATUS_LABEL[one.status]}</p>
    </header>

    {#if one.status === 'queued' && one.waiting}
      <p class="waiting">{waitLong(one.waiting, app.queueById(one.queue)?.name ?? 'Its queue')}</p>
    {/if}

    {#if one.status === 'failed' && one.error}
      <ErrorFix
        row={one}
        onfix={(fix) => app.fix(one.id, fix)}
        onretry={() => app.retry([one.id])}
      />
    {/if}

    {#if one.media}
      <p class="waiting">
        {one.media.quality}, {MEDIA_SOURCE_LABEL[one.media.source]}.
        {#if one.status === 'processing'}
          The video and audio are downloaded; Wirecatch is joining them into one file.
        {/if}
      </p>
    {/if}

    <section class="progress" aria-label="Progress">
      <p class="readout num">
        {#if one.status === 'completed'}
          {formatBytes(one.total ?? one.done)}
        {:else if one.speed > 0}
          {formatSpeed(one.speed)}
        {:else}
          {formatPercent(one.done, one.total) || formatBytes(one.done)}
        {/if}
      </p>
      <div class="wire {one.status}" aria-hidden="true">
        <span
          class="fill"
          style:transform="scaleX({one.status === 'completed' ? 1 : (fraction ?? 0)})"
        ></span>
      </div>
      <p class="facts num">
        <span>{formatSize(one.done, one.total, one.status)}</span>
        {#if one.status !== 'completed' && formatPercent(one.done, one.total)}
          <span>{formatPercent(one.done, one.total)} done</span>
        {/if}
        {#if one.eta_secs !== null && one.speed > 0}
          <span>{formatEta(one.eta_secs)} left</span>
        {/if}
      </p>
    </section>

    <div class="actions">
      {#if isRunning(one)}
        <button class="btn" onclick={() => app.pause([one.id])}
          ><Pause size={14} aria-hidden="true" /> Pause</button
        >
      {:else if one.status === 'paused'}
        <button class="btn primary" onclick={() => app.resume([one.id])}
          ><Play size={14} aria-hidden="true" /> Resume</button
        >
      {:else if one.status === 'cancelled'}
        <button class="btn primary" onclick={() => app.retry([one.id])}
          ><RotateCw size={14} aria-hidden="true" /> Start over</button
        >
      {:else if one.status === 'failed' && !one.error}
        <!-- With an error, the error box above already offers Try again. -->
        <button class="btn" onclick={() => app.retry([one.id])}
          ><RotateCw size={14} aria-hidden="true" /> Try again</button
        >
      {/if}
      {#if one.status === 'completed'}
        <button class="btn primary" onclick={() => app.openFile(one.id)}>Open</button>
      {/if}
      <button class="btn" onclick={() => app.openFolder(one.id)}
        ><FolderOpen size={14} aria-hidden="true" /> Show in folder</button
      >
      <button class="btn" onclick={() => app.showProgressWindow(one.id)}
        ><AppWindow size={14} aria-hidden="true" /> Show progress window</button
      >
      <button class="btn" onclick={() => app.openDialog({ kind: 'remove', ids: [one.id] })}>
        <Trash size={14} aria-hidden="true" /> Remove
      </button>
    </div>

    <dl>
      <div>
        <dt><label for="detail-queue">Queue</label></dt>
        <dd>
          <select id="detail-queue" class="field" value={one.queue} onchange={move}>
            {#each app.queues as q (q.id)}
              <option value={q.id}>{q.name}</option>
            {/each}
          </select>
        </dd>
      </div>
      <div>
        <dt>Connections</dt>
        <dd class="num">
          {#if one.connections > 0}
            {one.connections} open now, up to {app.settings?.connections_per_download ?? 8}
          {:else}
            Up to {app.settings?.connections_per_download ?? 8}
          {/if}
        </dd>
      </div>
      <div>
        <dt>Saved to</dt>
        <dd class="path">
          <span>{folderOf(one.path)}</span>
          {#if one.status !== 'completed'}
            <button class="btn small" onclick={() => app.chooseFolderFor(one.id)}>Change</button>
          {/if}
        </dd>
      </div>
      <div>
        <dt>Link</dt>
        <dd class="path">
          <span class="url">{linkForDisplay(one.url)}</span>
          <button
            class="icon-btn"
            aria-label="Copy link"
            title="Copy link"
            onclick={() => copyLink(linkForDisplay(one.url))}
          >
            <Copy size={14} aria-hidden="true" />
          </button>
        </dd>
      </div>
      {#if !['completed', 'failed', 'cancelled', 'processing'].includes(one.status)}
        <div>
          <dt>Link expired?</dt>
          <dd>
            <button
              class="btn small"
              onclick={() => app.openDialog({ kind: 'refresh', id: one.id })}
              >Give it a fresh link</button
            >
          </dd>
        </div>
      {/if}
      <div>
        <dt>Added</dt>
        <dd class="num">{formatDate(one.added_at, app.now)}</dd>
      </div>
      {#if one.completed_at !== null}
        <div>
          <dt>Finished</dt>
          <dd class="num">{formatDate(one.completed_at, app.now)}</dd>
        </div>
      {/if}
    </dl>
    {#if one.status !== 'completed' && one.status !== 'cancelled'}
      <button
        class="btn ghost small cancel"
        onclick={() => app.openDialog({ kind: 'cancel', ids: [one.id] })}
      >
        Cancel and discard the partial file
      </button>
    {/if}
  {:else if rows.length > 1}
    <header>
      <h2>{rows.length} downloads selected</h2>
      <p class="status num">{formatBytes(totalSize)} in total</p>
    </header>
    <div class="actions">
      {#if anyRunning}
        <button class="btn" onclick={() => app.pause()}
          ><Pause size={14} aria-hidden="true" /> Pause</button
        >
      {/if}
      {#if anyStopped}
        <button class="btn" onclick={() => app.resume()}
          ><Play size={14} aria-hidden="true" /> Resume</button
        >
      {/if}
      {#if anyFailed}
        <button class="btn" onclick={() => app.retry()}
          ><RotateCw size={14} aria-hidden="true" /> Try again</button
        >
      {/if}
      <button class="btn" onclick={() => app.openDialog({ kind: 'remove', ids: app.selectedIds })}>
        <Trash size={14} aria-hidden="true" /> Remove
      </button>
    </div>
    <dl>
      <div>
        <dt><label for="detail-queue-many">Move to queue</label></dt>
        <dd>
          <select id="detail-queue-many" class="field" value={sharedQueue ?? ''} onchange={move}>
            {#if sharedQueue === undefined}
              <option value="" disabled>Choose a queue</option>
            {/if}
            {#each app.queues as q (q.id)}
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

  .close {
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
    width: 316px;
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

  .readout {
    margin: 0 0 8px;
    font-size: var(--text-readout);
    font-weight: 600;
    font-variation-settings: 'wdth' 78;
    line-height: 1;
  }

  .wire {
    position: relative;
    height: 4px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--wire-track);
  }

  .wire .fill {
    position: absolute;
    inset: 0;
    background: var(--accent);
    transform-origin: left center;
    transition: transform 240ms linear;
  }

  .wire.paused .fill {
    background: var(--warn-wire);
  }

  .wire.failed .fill {
    background: var(--danger);
  }

  .wire.completed .fill,
  .wire.queued .fill {
    background: var(--wire-done);
  }

  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 14px;
    margin: 8px 0 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
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

  .cancel {
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
