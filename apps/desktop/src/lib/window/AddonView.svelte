<script lang="ts">
  // The media add-on offer, in the window of the video that needs it: a video page that cannot
  // be read without it, or a quality picked here whose video and audio must be joined. The
  // download goes on by itself once the add-on is installed.
  import type { AddonInfo } from '@wirecatch/protocol';
  import { addonAnnouncement, addonStatus, installFraction, isInstalled } from '$lib/addons';
  import Button from '$lib/components/Button.svelte';
  import ProgressWire from '$lib/components/ProgressWire.svelte';
  import { hostOf } from '$lib/format';
  import { track } from '$lib/state/reactivity';
  import { formatBytes } from '@wirecatch/units';
  import type { DownloadWindowState } from './download-window.svelte';
  import WindowFrame from './WindowFrame.svelte';

  /** `forQuality`: a quality picked in this window waits for it, rather than a page. */
  let {
    win,
    addon,
    url,
    forQuality = false,
  }: { win: DownloadWindowState; addon: AddonInfo; url: string; forQuality?: boolean } = $props();

  const installing = $derived(addon.state.type === 'installing');
  const installed = $derived(isInstalled(addon));
  const failed = $derived(addon.state.type === 'failed');
  const fraction = $derived(installFraction(addon.state));
  let busy = $state(false);

  async function install() {
    if (busy) return;
    busy = true;
    await win.installAddon();
    busy = false;
  }

  // The buttons change with the install. When the focused one goes away, the new main button
  // takes the focus instead of nothing.
  let body = $state<HTMLDivElement>();
  $effect(() => {
    track(addon.state.type);
    const frame = body?.closest('[role="group"]');
    const active = document.activeElement;
    if (frame && (!active || active === document.body)) {
      frame.querySelector<HTMLElement>('.foot [data-autofocus]')?.focus();
    }
  });
</script>

<WindowFrame
  title={installed
    ? `${addon.name} is installed`
    : url
      ? `Downloading from ${hostOf(url)} needs the ${addon.name.toLowerCase()}`
      : `This video needs the ${addon.name.toLowerCase()}`}
>
  <div class="body" bind:this={body}>
    <p>{addon.description}</p>
    <!-- Always mounted, so a screen reader hears the start, the progress and the end (or the
         failure) of the install. -->
    <p class="visually-hidden" aria-live="polite" data-testid="addon-live">
      {addonAnnouncement(addon)}
    </p>
    {#if installed}
      <p>Your download continues on its own.</p>
    {:else if installing}
      <p>{addonStatus(addon)}</p>
      <ProgressWire {fraction} label="Installing {addon.name}" />
      <p class="muted">
        {forQuality
          ? 'Your download starts here as soon as it is ready. Keep this window open.'
          : 'The video is read as soon as it is ready, and you pick its quality here. You can hide this window meanwhile.'}
      </p>
    {:else if failed}
      <p class="error">{addonStatus(addon)}</p>
    {:else}
      <p class="muted">
        One click installs it{addon.size === null ? '' : ` (${formatBytes(addon.size)})`}, then your
        download continues on its own.
      </p>
    {/if}
    {#if win.error}<p class="error" role="alert">{win.error}</p>{/if}
  </div>

  {#snippet footer()}
    {#if installing || installed}
      {#if installing}
        <Button onclick={() => win.stopAddonInstall()}>Stop installing</Button>
      {/if}
      <Button variant="primary" data-autofocus onclick={() => win.close()}>Hide</Button>
    {:else if !installed}
      <Button onclick={() => win.declineAddon()}
        >{forQuality ? 'Pick another quality' : 'Cancel'}</Button
      >
      <Button variant="primary" data-autofocus aria-disabled={busy} onclick={install}
        >{failed ? 'Try again' : 'Install'}</Button
      >
    {/if}
  {/snippet}
</WindowFrame>

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
</style>
