<script lang="ts">
  // The top of the sidebar: the name, and how fast everything is moving right now.
  import { PRODUCT_NAME } from '@wirecatch/product';
  import { formatSpeed } from '@wirecatch/units';
  import { getApp } from '$lib/state/app.svelte';

  const app = getApp();

  /** A phrase in the readout longer than this does not fit the column at the size of a speed. */
  const HEADLINE_FITS = 8;

  const speed = $derived(formatSpeed(app.downloads.totalSpeed));
  const active = $derived(app.counts.status.active);
  const waiting = $derived(app.counts.status.waiting);
  const joining = $derived(app.counts.joining);
  /** What the readout says when nothing is moving bytes. */
  const headline = $derived(
    active === 0 ? 'Idle' : joining === active ? 'Finishing videos' : 'Starting',
  );
</script>

<header class="readout" aria-live="off">
  <p class="brand">
    <svg viewBox="0 0 20 12" width="20" height="12" aria-hidden="true">
      <path
        d="M1 6h6l2-4 2 8 2-4h6"
        fill="none"
        stroke="currentColor"
        stroke-width="1.8"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
    {PRODUCT_NAME}
  </p>
  <p
    class="speed num"
    class:word={!speed && headline.length > HEADLINE_FITS}
    title="Total download speed"
  >
    {speed || headline}
  </p>
  <p class="sub num">
    {#if active + waiting === 0}
      Nothing running
    {:else}
      {[
        active - joining > 0 || joining === 0 ? `${active - joining} downloading` : '',
        joining > 0 ? `${joining} being joined or converted` : '',
        waiting > 0 ? `${waiting} waiting` : '',
      ]
        .filter(Boolean)
        .join(', ')}
    {/if}
  </p>
</header>

<style>
  p {
    margin: 0;
  }

  .readout {
    padding: 16px 16px 12px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--ink);
    font-size: var(--text-sm);
    font-weight: 650;
    letter-spacing: 0.01em;
  }

  .brand svg {
    color: var(--accent);
  }

  /* The one loud thing in the chrome: how fast everything is moving right now. */
  .speed {
    margin-top: 12px;
    color: var(--ink);
    font-size: var(--text-readout);
    font-weight: 600;
    font-variation-settings: 'wdth' 78;
    line-height: 1.05;
    letter-spacing: -0.01em;
  }

  /* A phrase instead of a number fits the column at a smaller size. */
  .speed.word {
    font-size: var(--text-lg);
    line-height: 1.3;
  }

  .sub {
    margin-top: 2px;
    font-size: var(--text-sm);
  }
</style>
