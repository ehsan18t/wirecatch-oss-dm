<script lang="ts">
  // The toolbar popup: the connection at a glance, capture on or off for every site and for this one, whether a window asks before each download, the media on this page, and the Pause all and Resume all buttons. Every other setting is on the options page, one click away (D5, D36).
  import { PRODUCT_NAME } from '@wirecatch/product';
  import { Callout } from '@wirecatch/ui';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import AccessNotice from '../common/AccessNotice.svelte';
  import ConnectionNotice from '../common/ConnectionNotice.svelte';
  import { cachedNote, waitingNote } from '../connection';
  import CaptureCard from './CaptureCard.svelte';
  import MediaList from './MediaList.svelte';
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
    /** Opens the options page. */
    openSettings?: () => void;
  }

  let {
    model,
    currentUrl,
    siteAccess = true,
    requestSiteAccess,
    mediaShortcut = null,
    openSettings = () => undefined,
  }: Props = $props();

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
  const cached = $derived(cachedNote(snapshot));
</script>

<svelte:head><title>{PRODUCT_NAME}</title></svelte:head>

<!-- Browsers stop a popup at 600px: only the middle scrolls, so the state and the Pause all and Resume all buttons stay in view. -->
<div class="flex max-h-popup w-popup flex-col">
  <PopupHeader {status} onsettings={openSettings} />

  <main
    aria-busy={snapshot === null}
    tabindex="-1"
    class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto overscroll-contain px-3 pb-3"
    bind:this={mainArea}
  >
    {#if problem !== null}
      <ConnectionNotice
        {problem}
        note={cached}
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
    {#if cached !== null && problem === null}
      <Callout tone="neutral">{cached}</Callout>
    {/if}

    {#if model.settings === null}
      <!-- With nothing to show (never connected, no cache), say so instead of showing made-up values. -->
      <p class="px-1 text-ui text-ink-muted">{waitingNote(status)}</p>
    {:else}
      <CaptureCard {model} settings={model.settings} {currentUrl} />
    {/if}

    <MediaList
      view={model.media}
      statuses={model.mediaStatus}
      buttonOn={model.settings?.show_media_button ?? null}
      shortcut={mediaShortcut}
      ondownload={(key: string) => void model.downloadMedia(key)}
      onsettings={openSettings}
    />
  </main>

  <PopupFooter
    connected={model.connected}
    feedback={model.feedback}
    onpause={() => void model.pauseAll()}
    onresume={() => void model.resumeAll()}
  />
</div>
