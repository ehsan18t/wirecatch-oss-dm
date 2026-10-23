<script lang="ts">
  // The toolbar popup: the connection at a glance, capture on or off for every site and for this one, whether a window asks before each download and each video, the media on this page, the settings someone may change while browsing folded under Downloads and Video and audio, and the Pause all and Resume all buttons. The settings are the app's, shown only while it is connected; the rest are in the app, behind the settings button (D61).
  import { PRODUCT_NAME } from '@wirecatch/product';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import AccessNotice from '../common/AccessNotice.svelte';
  import ConnectionNotice from '../common/ConnectionNotice.svelte';
  import { waitingNote } from '../connection';
  import CaptureCard from './CaptureCard.svelte';
  import DownloadsSection from './DownloadsSection.svelte';
  import MediaList from './MediaList.svelte';
  import MediaSection from './MediaSection.svelte';
  import Notices from './Notices.svelte';
  import PopupFooter from './PopupFooter.svelte';
  import PopupHeader from './PopupHeader.svelte';

  interface Props {
    model: PopupModel;
    /** Address of the active tab, for "Ignore this site". */
    currentUrl: string | null;
    /** False when the person revoked the extension's access to websites (possible in Firefox). */
    siteAccess?: boolean;
    /** Asks the browser to grant website access again; resolves with the new state. */
    requestSiteAccess?: () => Promise<boolean>;
    /** The keyboard shortcut that downloads a page's media; null when none is set. */
    mediaShortcut?: string | null;
    /** Opens the browser's page of extension shortcuts; null where no page can open it (Firefox). */
    openShortcuts?: (() => void) | null;
    /** Closes the popup, once the app's settings are opening. */
    close?: () => void;
  }

  let {
    model,
    currentUrl,
    siteAccess = true,
    requestSiteAccess,
    mediaShortcut = null,
    openShortcuts = null,
    close = () => undefined,
  }: Props = $props();

  async function openSettings(): Promise<void> {
    if (await model.openSettings()) close();
  }

  let accessGranted = $state<boolean | null>(null);
  const hasSiteAccess = $derived(accessGranted ?? siteAccess);

  async function allowAccess(): Promise<void> {
    if (requestSiteAccess === undefined) return;
    accessGranted = await requestSiteAccess();
  }

  let mainArea = $state<HTMLElement | null>(null);
  const snapshot = $derived(model.snapshot);
  const status = $derived(snapshot?.status ?? { state: 'connecting' as const });
  const problem = $derived(status.state === 'unavailable' ? status.problem : null);
</script>

<svelte:head><title>{PRODUCT_NAME}</title></svelte:head>

<!-- Browsers stop a popup at 600px: only the middle scrolls, so the state and the Pause all and Resume all buttons stay in view. -->
<div class="flex max-h-popup w-popup flex-col">
  <PopupHeader {status} onsettings={() => void openSettings()} />

  <main
    aria-busy={snapshot === null}
    tabindex="-1"
    class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto overscroll-contain px-3 pb-3"
    bind:this={mainArea}
  >
    {#if problem !== null}
      <ConnectionNotice
        {problem}
        onopen={() => void model.openApp()}
        onretry={() => {
          model.reconnect();
        }}
      />
    {/if}
    {#if !hasSiteAccess}
      <AccessNotice onallow={allowAccess} />
    {/if}
    <Notices
      notices={snapshot?.notices ?? []}
      ondismiss={(id: string) => {
        model.dismissNotice(id);
      }}
      fallback={() => mainArea}
    />

    {#if model.settings === null}
      <!-- The settings are the app's: while it is not connected, say so instead of showing values it may have changed. -->
      <p class="px-1 text-ui text-ink-muted">{waitingNote(status)}</p>
    {:else}
      <CaptureCard {model} settings={model.settings} controls={model.controls} {currentUrl} />
    {/if}

    <MediaList
      view={model.media}
      statuses={model.mediaStatus}
      shortcut={mediaShortcut}
      {openShortcuts}
      buttonOn={model.editable ? (model.settings?.show_media_button ?? null) : null}
      ontogglebutton={(on: boolean) => void model.update({ show_media_button: on })}
      ondownload={(key: string) => void model.downloadMedia(key)}
    />

    {#if model.settings !== null}
      <DownloadsSection
        {model}
        settings={model.settings}
        controls={model.controls}
        queues={snapshot?.queues ?? []}
      />
      <MediaSection {model} settings={model.settings} controls={model.controls} />
    {/if}
  </main>

  <PopupFooter
    connected={model.connected}
    feedback={model.feedback}
    onpause={() => void model.pauseAll()}
    onresume={() => void model.resumeAll()}
  />
</div>
