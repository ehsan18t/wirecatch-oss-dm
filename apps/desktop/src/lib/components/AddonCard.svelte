<script lang="ts">
  import type { AddonInfo } from '@wirecatch/protocol';
  import {
    addonAction,
    addonAnnouncement,
    addonStatus,
    installFraction,
    isInstalled,
  } from '$lib/addons';
  import { getApp } from '$lib/state/app.svelte';

  let { addon, describedby }: { addon: AddonInfo; describedby?: string } = $props();
  const app = getApp();

  const action = $derived(addonAction(addon));
  const fraction = $derived(installFraction(addon.state));
  const installing = $derived(addon.state.type === 'installing');
  let busy = $state(false);

  async function run(work: () => Promise<unknown>) {
    busy = true;
    await work();
    busy = false;
  }

  const ACTION_LABEL = { install: 'Install', update: 'Update', retry: 'Try again' } as const;
</script>

<div class="addon">
  <p class="status" class:failed={addon.state.type === 'failed'}>
    {addonStatus(addon)}
  </p>
  <p class="visually-hidden" aria-live="polite">{addonAnnouncement(addon)}</p>
  {#if installing}
    <div
      class="wire"
      role="progressbar"
      aria-label="Installing {addon.name}"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={fraction === null ? undefined : Math.floor(fraction * 100)}
    >
      <span
        class="fill"
        class:unknown={fraction === null}
        style:transform={fraction === null ? undefined : `scaleX(${fraction})`}
      ></span>
    </div>
  {/if}
  <div class="buttons">
    {#if action}
      <button
        class="btn small primary"
        aria-describedby={describedby}
        disabled={busy}
        onclick={() =>
          run(() => (action === 'update' ? app.updateAddon(addon.id) : app.installAddon(addon.id)))}
      >
        {ACTION_LABEL[action]}
      </button>
    {/if}
    {#if isInstalled(addon)}
      <button
        class="btn small"
        disabled={busy}
        aria-label="Remove the {addon.name.toLowerCase()}"
        onclick={() => run(() => app.removeAddon(addon.id))}>Remove</button
      >
    {/if}
  </div>
</div>

<style>
  .addon {
    display: grid;
    gap: 6px;
    justify-items: end;
    min-width: 220px;
  }

  .status {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
    text-align: right;
  }

  .status.failed {
    color: var(--danger);
  }

  .wire {
    position: relative;
    width: 220px;
    height: 4px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--wire-track);
  }

  .fill {
    position: absolute;
    inset: 0;
    background: var(--accent);
    transform-origin: left;
    transition: transform 200ms var(--ease);
  }

  .fill.unknown {
    width: 30%;
    animation: slide 1.2s var(--ease) infinite;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }

  .buttons {
    display: flex;
    gap: 6px;
  }
</style>
