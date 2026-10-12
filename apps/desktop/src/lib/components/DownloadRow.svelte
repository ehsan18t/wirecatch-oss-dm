<script lang="ts">
  import type { DownloadId } from '@wirecatch/protocol';
  import {
    FIX_LABEL,
    FIX_SHORT,
    formatEta,
    formatPercent,
    formatSize,
    statusShort,
    wireFraction,
  } from '$lib/format';
  import { formatSpeed } from '@wirecatch/units';
  import { getApp } from '$lib/state/app.svelte';
  import Button from './Button.svelte';
  import CategoryIcon from './CategoryIcon.svelte';
  import ProgressWire from './ProgressWire.svelte';

  let {
    id,
    index,
    dropBefore = false,
  }: { id: DownloadId; index: number; dropBefore?: boolean } = $props();

  const app = getApp();

  // The row object is reactive per field: only this row re-renders when its numbers change.
  const row = $derived(app.downloads.get(id));
  const selected = $derived(app.selection.has(id));
  const active = $derived(app.selection.active === id);
  const statusText = $derived(row ? statusShort(row) : '');
</script>

{#if row}
  <div
    id="row-{id}"
    class="row {row.status}"
    class:selected
    class:active
    class:drop-before={dropBefore}
    role="row"
    aria-selected={selected}
    aria-rowindex={index + 1}
    data-id={id}
    draggable="true"
  >
    <span class="icon" role="gridcell"><CategoryIcon category={row.category} /></span>
    <span class="name" role="gridcell" title={row.file_name} data-menu-anchor>{row.file_name}</span>
    {#if row.status === 'failed' && row.error}
      <span class="error" role="gridcell">
        <span class="message" title={row.error.message}>{row.error.message}</span>
        {#if row.error.fix}
          {@const fix = row.error.fix}
          <!-- Reached by mouse here; by keyboard, Enter on the row runs the same fix. -->
          <Button
            small
            class="fix"
            tabindex={-1}
            title={FIX_LABEL[fix]}
            onclick={() => app.actions.fix(row.id, fix)}
          >
            {FIX_SHORT[fix]}
          </Button>
        {/if}
      </span>
    {:else}
      <span class="size num" role="gridcell">{formatSize(row.done, row.total, row.status)}</span>
      <span class="pct num" role="gridcell"
        >{row.status === 'completed' ? '' : formatPercent(row.done, row.total)}</span
      >
      <span
        class="speed num"
        role="gridcell"
        title={row.connections > 0 ? `${row.connections} connections` : undefined}
      >
        {formatSpeed(row.speed)}
      </span>
      <span class="eta num" role="gridcell">{formatEta(row.eta_secs)}</span>
    {/if}
    <span class="status" role="gridcell" title={statusText}>{statusText}</span>
    <span class="wire"><ProgressWire fraction={wireFraction(row)} status={row.status} /></span>
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
  :global(.list:focus-visible) .row.active,
  :global(.list.menu-open) .row.active {
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
    display: flex;
    grid-column: 3 / -2;
    align-items: center;
    gap: 8px;
    min-width: 0;
    color: var(--danger);
    font-size: var(--text-sm);
  }

  .message {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .error :global(.fix) {
    flex: none;
    height: 24px;
    color: var(--ink);
  }

  /* Narrow windows drop time left first, then speed; the detail panel still shows both. */
  @container main (max-width: 899px) {
    .eta {
      display: none;
    }
  }

  @container main (max-width: 759px) {
    .speed {
      display: none;
    }
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
    --wire-height: 2px;
    position: absolute;
    right: 16px;
    bottom: 0;
    left: 18px;
  }

  .completed .wire {
    opacity: 0.45;
  }
</style>
