<script lang="ts">
  import type { DownloadId } from '@wirecatch/protocol';
  import {
    formatEta,
    formatPercent,
    formatSize,
    formatSpeed,
    progressFraction,
    STATUS_LABEL,
  } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import CategoryIcon from './CategoryIcon.svelte';

  let {
    id,
    index,
    setSize,
    dropBefore = false,
  }: { id: DownloadId; index: number; setSize: number; dropBefore?: boolean } = $props();

  const app = getApp();

  // The row object is reactive per field: only this row re-renders when its numbers change.
  const row = $derived(app.downloads.get(id));
  const selected = $derived(app.selection.has(id));
  const active = $derived(app.active === id);
  const fraction = $derived(row ? progressFraction(row.done, row.total) : null);
</script>

{#if row}
  <div
    id="row-{id}"
    class="row {row.status}"
    class:selected
    class:active
    class:drop-before={dropBefore}
    role="option"
    aria-selected={selected}
    aria-posinset={index + 1}
    aria-setsize={setSize}
    data-id={id}
    draggable="true"
  >
    <span class="icon"><CategoryIcon category={row.category} /></span>
    <span class="name" title={row.file_name}>{row.file_name}</span>
    {#if row.status === 'failed' && row.error}
      <span class="error">{row.error.message}</span>
    {:else}
      <span class="size num">{formatSize(row.done, row.total, row.status)}</span>
      <span class="pct num"
        >{row.status === 'completed' ? '' : formatPercent(row.done, row.total)}</span
      >
      <span
        class="speed num"
        title={row.connections > 0 ? `${row.connections} connections` : undefined}
      >
        {formatSpeed(row.speed)}
      </span>
      <span class="eta num">{formatEta(row.eta_secs)}</span>
    {/if}
    <span class="status">{STATUS_LABEL[row.status]}</span>
    <span class="wire" aria-hidden="true">
      {#if fraction === null && (row.status === 'downloading' || row.status === 'connecting')}
        <span class="fill unknown"></span>
      {:else}
        <span
          class="fill"
          style:transform="scaleX({row.status === 'completed' ? 1 : (fraction ?? 0)})"
        ></span>
      {/if}
    </span>
  </div>
{/if}

<style>
  .row {
    position: relative;
    display: grid;
    grid-template-columns: var(--list-columns);
    align-items: center;
    column-gap: 12px;
    height: var(--row-h);
    padding: 0 16px 0 18px;
    /* A row's layout never affects anything outside it. */
    contain: layout style;
    color: var(--ink);
    cursor: default;
    user-select: none;
  }

  .row:hover {
    background: var(--surface-2);
  }

  .row.selected {
    background: var(--accent-soft);
  }

  .row.selected::before {
    content: '';
    position: absolute;
    inset: 0 auto 0 0;
    width: 3px;
    background: var(--accent);
  }

  /* Keyboard position: only drawn while the list itself has keyboard focus. */
  :global(.list:focus-visible) .row.active {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }

  .row.drop-before::after {
    content: '';
    position: absolute;
    inset: -1px 0 auto 0;
    height: 2px;
    background: var(--accent);
  }

  .icon {
    display: grid;
    place-items: center;
    color: var(--ink-2);
  }

  .name {
    min-width: 0;
    overflow: hidden;
    font-weight: 520;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .size,
  .pct,
  .speed,
  .eta {
    overflow: hidden;
    color: var(--ink-2);
    font-size: var(--text-sm);
    text-align: right;
    white-space: nowrap;
  }

  .pct {
    color: var(--ink);
  }

  .speed {
    color: var(--accent-text);
    font-weight: 600;
  }

  .error {
    grid-column: 3 / 7;
    min-width: 0;
    overflow: hidden;
    color: var(--danger);
    font-size: var(--text-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .status {
    overflow: hidden;
    color: var(--ink-2);
    font-size: var(--text-sm);
    white-space: nowrap;
  }

  .downloading .status,
  .connecting .status {
    color: var(--accent-text);
  }

  .paused .status {
    color: var(--warn);
  }

  .cancelled .name {
    color: var(--ink-2);
    text-decoration: line-through;
    text-decoration-color: var(--line-strong);
  }

  .failed .status {
    color: var(--danger);
    font-weight: 600;
  }

  .completed .name {
    font-weight: 450;
  }

  /* The wire: each row's bottom edge is its progress bar. */
  .wire {
    position: absolute;
    right: 16px;
    bottom: 0;
    left: 18px;
    height: 2px;
    overflow: hidden;
    background: var(--wire-track);
  }

  .fill {
    position: absolute;
    inset: 0;
    background: var(--accent);
    transform-origin: left center;
    transition: transform 240ms linear;
  }

  .paused .fill {
    background: var(--warn-wire);
  }

  .failed .fill {
    background: var(--danger);
  }

  .queued .fill,
  .completed .fill {
    background: var(--wire-done);
  }

  .completed .wire {
    opacity: 0.45;
  }

  .fill.unknown {
    right: auto;
    width: 30%;
    animation: travel 1.6s var(--ease) infinite;
  }

  @keyframes travel {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
</style>
