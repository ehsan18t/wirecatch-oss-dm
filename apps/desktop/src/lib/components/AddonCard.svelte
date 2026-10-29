<script lang="ts">
  // An add-on's state and what can be done with it: one main button that installs, updates, tries again or stops, so the keyboard focus stays on it as the state changes, and the checks and removal once installed, on one line with the state. `align` is where it sits: at a settings row's end, or after text in a dialog.
  import type { AddonInfo } from '@wirecatch/protocol';
  import { formatBytes } from '@wirecatch/units';
  import { Button, cn, ProgressBar } from '@wirecatch/ui';
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

  let {
    addon,
    describedby,
    align = 'end',
  }: { addon: AddonInfo; describedby?: string; align?: 'start' | 'end' } = $props();
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
  /** The main button's words: what it does, with the download size when installing. */
  const mainLabel = $derived.by(() => {
    if (installing) return 'Stop';
    if (action === 'install' && addon.size !== null) return `Install (${formatBytes(addon.size)})`;
    return action ? ACTION_LABEL[action] : '';
  });
</script>

<div
  class={cn(
    'flex min-w-56 flex-col gap-2',
    align === 'end' ? 'items-end text-right' : 'items-start',
  )}
  bind:this={card}
  onfocusin={() => (hadFocus = true)}
  onfocusout={(e) => {
    // A button removed while focused leaves no new target; that still counts as inside.
    if (e.relatedTarget instanceof Node && !card?.contains(e.relatedTarget)) hadFocus = false;
  }}
>
  <div class={cn('flex flex-wrap items-center gap-x-3 gap-y-2', align === 'end' && 'justify-end')}>
    <p class={cn('text-small', addon.state.type === 'failed' ? 'text-danger' : 'text-ink-muted')}>
      {addonStatus(addon)}
    </p>
    {#if isInstalled(addon)}
      <Button
        size="sm"
        variant="ghost"
        aria-disabled={busy}
        onclick={() => run(() => app.addons.checkUpdates())}>Check for updates</Button
      >
      <Button
        size="sm"
        variant="danger-quiet"
        aria-disabled={busy}
        aria-label="Remove the {addon.name.toLowerCase()}"
        onclick={() => run(() => app.addons.remove(addon.id))}>Remove</Button
      >
    {/if}
    {#if action !== null || installing}
      <Button
        size="sm"
        variant={installing ? 'secondary' : 'primary'}
        aria-describedby={describedby}
        aria-disabled={busy}
        aria-label={installing
          ? `Stop ${addon.installed_version ? 'updating' : 'installing'} the ${addon.name.toLowerCase()}`
          : undefined}
        onclick={main}
      >
        {mainLabel}
      </Button>
    {/if}
  </div>
  {#if addon.note}
    <p class="max-w-80 text-small text-ink-muted">{addon.note}</p>
  {/if}
  <p class="sr-only" aria-live="polite">{addonAnnouncement(addon)}</p>
  {#if installing}
    <ProgressBar value={fraction} size="sm" label="Installing {addon.name}" class="w-56" />
  {/if}
</div>
