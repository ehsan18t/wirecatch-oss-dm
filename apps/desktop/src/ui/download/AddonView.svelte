<script lang="ts">
  // The media add-on offer, in the window of the video that needs it: a video page that cannot be read without it, or a quality picked here whose video and audio must be joined. The download goes on by itself once the add-on is installed.
  import type { AddonInfo } from '@wirecatch/protocol';
  import { formatBytes } from '@wirecatch/units';
  import { Button, Callout, ProgressBar } from '@wirecatch/ui';
  import { addonAnnouncement, addonStatus, installFraction, isInstalled } from '$lib/addons';
  import { hostOf } from '$lib/format';
  import type { DownloadWindowState } from '$lib/window/download-window.svelte';
  import WindowFrame from './WindowFrame.svelte';

  interface Props {
    win: DownloadWindowState;
    addon: AddonInfo;
    url: string;
    /** A quality picked in this window waits for it, rather than a page. */
    forQuality?: boolean;
  }

  let { win, addon, url, forQuality = false }: Props = $props();

  const installing = $derived(addon.state.type === 'installing');
  const installed = $derived(isInstalled(addon));
  const failed = $derived(addon.state.type === 'failed');
  const fraction = $derived(installFraction(addon.state));
  const addonName = $derived(addon.name.toLowerCase());
  const title = $derived.by(() => {
    if (installed) return `${addon.name} is installed`;
    return url
      ? `Downloading from ${hostOf(url)} needs the ${addonName}`
      : `This video needs the ${addonName}`;
  });
  let busy = $state(false);

  async function install() {
    if (busy) return;
    busy = true;
    await win.installAddon();
    busy = false;
  }
</script>

<WindowFrame {title} error={win.error}>
  <div class="flex flex-col gap-3 text-body text-ink">
    <p>{addon.description}</p>
    <!-- Always there, so a screen reader hears the start, the progress and the end (or the failure) of the install: a region added together with its text is often not read. -->
    <p class="sr-only" aria-live="polite" data-testid="addon-live">{addonAnnouncement(addon)}</p>
    {#if installed}
      <p>Your download continues on its own.</p>
    {:else if installing}
      <p>{addonStatus(addon)}</p>
      <ProgressBar value={fraction} label="Installing {addon.name}" />
      <p class="text-ui text-ink-muted">
        {forQuality
          ? 'Your download starts here as soon as it is ready. Keep this window open.'
          : 'The video is read as soon as it is ready, and you pick its quality here. You can hide this window meanwhile.'}
      </p>
    {:else if failed}
      <Callout tone="danger" title="The add-on did not install">{addonStatus(addon)}</Callout>
    {:else}
      <p class="text-ui text-ink-muted">
        One click installs it{addon.size === null ? '' : ` (${formatBytes(addon.size)})`}, then your
        download continues on its own.
      </p>
    {/if}
  </div>

  {#snippet footer()}
    {#if installing || installed}
      {#if installing}
        <Button variant="ghost" onclick={() => void win.stopAddonInstall()}>Stop installing</Button>
      {/if}
      <Button variant="primary" data-autofocus onclick={() => win.close()}>Hide</Button>
    {:else}
      <Button variant="ghost" onclick={() => void win.declineAddon()}
        >{forQuality ? 'Pick another quality' : 'Cancel'}</Button
      >
      <Button variant="primary" data-autofocus loading={busy} onclick={() => void install()}
        >{failed ? 'Try again' : 'Install'}</Button
      >
    {/if}
  {/snippet}
</WindowFrame>
