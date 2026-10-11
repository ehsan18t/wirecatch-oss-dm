<script lang="ts">
  import type { AddonInfo } from '@wirecatch/protocol';
  import { addonAnnouncement, addonStatus, installFraction, isInstalled } from '$lib/addons';
  import { formatBytes, hostOf } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import Dialog from './Dialog.svelte';

  let { addon, url }: { addon: AddonInfo; url: string } = $props();
  const app = getApp();

  const installing = $derived(addon.state.type === 'installing');
  const installed = $derived(isInstalled(addon));
  const failed = $derived(addon.state.type === 'failed');
  const fraction = $derived(installFraction(addon.state));
  let busy = $state(false);

  async function install() {
    if (busy) return;
    busy = true;
    await app.installAddon(addon.id);
    busy = false;
  }

  // The footer's buttons change with the install (Install becomes Close, then Done). When the
  // focused one goes away, the new main button takes the focus instead of nothing.
  let body = $state<HTMLDivElement>();
  $effect(() => {
    void addon.state.type;
    const dialog = body?.closest('dialog');
    const active = document.activeElement;
    if (dialog && (!active || active === document.body)) {
      dialog.querySelector<HTMLElement>('footer [data-autofocus]')?.focus();
    }
  });
</script>

<Dialog
  title={installed
    ? `${addon.name} is installed`
    : url
      ? `Downloading from ${hostOf(url)} needs the ${addon.name.toLowerCase()}`
      : `This video needs the ${addon.name.toLowerCase()}`}
  onclose={() => app.closeAddonRequest()}
  width={460}
>
  <div class="body" bind:this={body}>
    <p>{addon.description}</p>
    {#if installed}
      <p>Your download continues on its own.</p>
    {:else if installing}
      <p>{addonStatus(addon)}</p>
      <p class="visually-hidden" aria-live="polite">{addonAnnouncement(addon)}</p>
      <div
        class="wire"
        role="progressbar"
        aria-label="Installing {addon.name}"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={fraction === null ? undefined : Math.floor(fraction * 100)}
      >
        <span class="fill" style:transform="scaleX({fraction ?? 0.05})"></span>
      </div>
      <p class="muted">Your download starts as soon as it is ready. You can close this.</p>
    {:else if failed}
      <p class="error" role="alert">{addonStatus(addon)}</p>
    {:else}
      <p class="muted">
        One click installs it{addon.size === null ? '' : ` (${formatBytes(addon.size)})`}, then your
        download starts on its own.
      </p>
    {/if}
  </div>

  {#snippet footer()}
    {#if installed || installing}
      <button
        type="button"
        class="btn primary"
        data-autofocus
        onclick={() => app.closeAddonRequest()}
        >{installed ? 'Done' : 'Close and keep installing'}</button
      >
    {:else}
      <button type="button" class="btn" onclick={() => app.closeAddonRequest()}>Not now</button>
      <button
        type="button"
        class="btn primary"
        data-autofocus
        aria-disabled={busy}
        onclick={install}>{failed ? 'Try again' : 'Install'}</button
      >
    {/if}
  {/snippet}
</Dialog>

<style>
  .body {
    display: grid;
    gap: 10px;
  }

  p {
    margin: 0;
    line-height: 1.5;
  }

  .muted {
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .error {
    color: var(--danger);
    font-size: var(--text-sm);
  }

  .wire {
    position: relative;
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
</style>
