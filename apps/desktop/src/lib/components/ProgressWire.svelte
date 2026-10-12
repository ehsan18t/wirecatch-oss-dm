<script lang="ts">
  // The progress wire: how far a download or an add-on install has got, the same everywhere it
  // shows. A fraction fills it from the left; `null` means how far is not known yet, and a short
  // piece then travels along it. A download's status colours it. With a label it is a progress
  // bar for screen readers; without one it is decoration next to words that say the same.
  // Its thickness is the `--wire-height` of the element it sits in (4px unless set).
  import type { DownloadStatus } from '@wirecatch/protocol';
  import { progressbarValue } from '$lib/format';

  let {
    fraction,
    status = null,
    label = null,
  }: {
    /** From 0 to 1, or `null` while unknown. */
    fraction: number | null;
    /** The download's status, which picks the colour; none for an add-on. */
    status?: DownloadStatus | null;
    /** What a screen reader calls the bar; without it the wire is hidden from them. */
    label?: string | null;
  } = $props();

  const a11y = $derived(
    label === null
      ? { 'aria-hidden': true }
      : {
          role: 'progressbar',
          'aria-label': label,
          ...progressbarValue(fraction, status === 'completed'),
        },
  );
</script>

<span class="wire {status ?? ''}" {...a11y}>
  {#if fraction === null}
    <span class="fill unknown"></span>
  {:else}
    <span class="fill" style:transform="scaleX({fraction})"></span>
  {/if}
</span>

<style>
  .wire {
    display: block;
    position: relative;
    height: var(--wire-height, 4px);
    overflow: hidden;
    border-radius: calc(var(--wire-height, 4px) / 2);
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

  .fill.unknown {
    right: auto;
    width: 30%;
    animation: travel 1.4s var(--ease) infinite;
  }

  /* Without motion, a dim full wire says "under way, how far unknown". */
  @media (prefers-reduced-motion: reduce) {
    .fill.unknown {
      width: 100%;
      opacity: 0.35;
      animation: none;
    }
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
