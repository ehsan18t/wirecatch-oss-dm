<script lang="ts">
  // How far one download has got, in the detail panel: the speed or size in large type, the wire,
  // and what is done and what is left.
  import type { DownloadSummary } from '@wirecatch/protocol';
  import { formatBytes, formatSpeed } from '@wirecatch/units';
  import {
    formatEta,
    formatPercent,
    formatSize,
    processingFraction,
    progressFraction,
  } from '$lib/format';

  let { row }: { row: DownloadSummary } = $props();

  const fraction = $derived(processingFraction(row) ?? progressFraction(row.done, row.total));
  const percent = $derived(formatPercent(row.done, row.total));
</script>

<section class="progress" aria-label="Progress">
  <p class="readout num">
    {#if row.status === 'completed'}
      {formatBytes(row.total ?? row.done)}
    {:else if row.speed > 0}
      {formatSpeed(row.speed)}
    {:else}
      {percent || formatBytes(row.done)}
    {/if}
  </p>
  <div class="wire {row.status}" aria-hidden="true">
    <span class="fill" style:transform="scaleX({row.status === 'completed' ? 1 : (fraction ?? 0)})"
    ></span>
  </div>
  <p class="facts num">
    <span>{formatSize(row.done, row.total, row.status)}</span>
    {#if row.status !== 'completed' && percent}
      <span>{percent} done</span>
    {/if}
    {#if row.eta_secs !== null && row.speed > 0}
      <span>{formatEta(row.eta_secs)} left</span>
    {/if}
  </p>
</section>

<style>
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
</style>
