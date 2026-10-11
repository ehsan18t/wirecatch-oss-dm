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
    if (busy) return;
    busy = true;
    await work();
    busy = false;
  }

  function main() {
    // Stopping never removes a working add-on: an update keeps the installed version.
    if (installing) return run(() => app.cancelAddonOperation(addon.id));
    return run(() =>
      action === 'update' ? app.updateAddon(addon.id) : app.installAddon(addon.id),
    );
  }

  // When the focused button goes away (Install once installed, Stop once stopped), the focus
  // moves to the card's other button instead of being lost.
  let card = $state<HTMLDivElement>();
  let hadFocus = false;
  $effect(() => {
    void addon.state.type;
    const active = document.activeElement;
    if (hadFocus && card && (!active || active === document.body)) {
      card.querySelector<HTMLElement>('button')?.focus();
    }
  });

  const ACTION_LABEL = { install: 'Install', update: 'Update', retry: 'Try again' } as const;
</script>

<div
  class="addon"
  bind:this={card}
  onfocusin={() => (hadFocus = true)}
  onfocusout={(e) => {
    // A button removed while focused leaves no new target; that still counts as inside.
    if (e.relatedTarget instanceof Node && !card?.contains(e.relatedTarget)) hadFocus = false;
  }}
>
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
    <!-- One button for install, update, try again and stop, so the keyboard focus stays on it
         as the state changes. -->
    {#if action || installing}
      <button
        class="btn small"
        class:primary={!installing}
        aria-describedby={describedby}
        aria-disabled={busy}
        aria-label={installing
          ? `Stop ${addon.installed_version ? 'updating' : 'installing'} the ${addon.name.toLowerCase()}`
          : undefined}
        onclick={main}
      >
        {installing ? 'Stop' : action ? ACTION_LABEL[action] : ''}
      </button>
    {/if}
    {#if isInstalled(addon)}
      <button
        class="btn small"
        aria-disabled={busy}
        onclick={() => run(() => app.checkAddonUpdates())}>Check for updates</button
      >
      <button
        class="btn small"
        aria-disabled={busy}
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
