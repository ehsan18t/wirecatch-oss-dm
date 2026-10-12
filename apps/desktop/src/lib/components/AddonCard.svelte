<script lang="ts">
  import type { AddonInfo } from '@wirecatch/protocol';
  import {
    addonAction,
    addonAnnouncement,
    addonStatus,
    installFraction,
    isInstalled,
  } from '$lib/addons';
  import { findElement } from '$lib/dom';
  import { getApp } from '$lib/state/app.svelte';
  import { track } from '$lib/state/reactivity';
  import Button from './Button.svelte';
  import ProgressWire from './ProgressWire.svelte';

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
    if (installing) return run(() => app.addons.cancel(addon.id));
    return run(() =>
      action === 'update' ? app.addons.update(addon.id) : app.addons.install(addon.id),
    );
  }

  // When the focused button goes away (Install once installed, Stop once stopped), the focus
  // moves to the card's other button instead of being lost.
  let card = $state<HTMLDivElement>();
  let hadFocus = false;
  $effect(() => {
    track(addon.state.type);
    const active = document.activeElement;
    if (hadFocus && card && (!active || active === document.body)) {
      findElement(card, 'button')?.focus();
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
  {#if addon.note}
    <p class="note">{addon.note}</p>
  {/if}
  <p class="visually-hidden" aria-live="polite">{addonAnnouncement(addon)}</p>
  {#if installing}
    <div class="install">
      <ProgressWire {fraction} label="Installing {addon.name}" />
    </div>
  {/if}
  <div class="buttons">
    <!-- One button for install, update, try again and stop, so the keyboard focus stays on it
         as the state changes. -->
    {#if action !== null || installing}
      <Button
        small
        variant={installing ? 'default' : 'primary'}
        aria-describedby={describedby}
        aria-disabled={busy}
        aria-label={installing
          ? `Stop ${addon.installed_version ? 'updating' : 'installing'} the ${addon.name.toLowerCase()}`
          : undefined}
        onclick={main}
      >
        {installing ? 'Stop' : action ? ACTION_LABEL[action] : ''}
      </Button>
    {/if}
    {#if isInstalled(addon)}
      <Button small aria-disabled={busy} onclick={() => run(() => app.addons.checkUpdates())}
        >Check for updates</Button
      >
      <Button
        small
        aria-disabled={busy}
        aria-label="Remove the {addon.name.toLowerCase()}"
        onclick={() => run(() => app.addons.remove(addon.id))}>Remove</Button
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

  .note {
    max-width: 320px;
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
    text-align: right;
  }

  .install {
    width: 220px;
  }

  .buttons {
    display: flex;
    gap: 6px;
  }
</style>
