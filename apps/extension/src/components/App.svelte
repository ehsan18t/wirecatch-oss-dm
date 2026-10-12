<script lang="ts">
  import { PRODUCT_NAME } from '@wirecatch/product';
  import type { ExtensionSettings } from '@wirecatch/protocol';
  import { describeProblem } from '../lib/native/status';
  import type { PopupModel } from '../lib/popup/model.svelte';
  import MediaSection from './MediaSection.svelte';
  import PopupHeader from './PopupHeader.svelte';
  import SettingsForm from './SettingsForm.svelte';

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
  }

  let {
    model,
    currentUrl,
    siteAccess = true,
    requestSiteAccess,
    mediaShortcut = null,
  }: Props = $props();

  let accessGranted = $state<boolean | null>(null);
  const hasSiteAccess = $derived(accessGranted ?? siteAccess);

  async function allowAccess(): Promise<void> {
    if (requestSiteAccess === undefined) return;
    accessGranted = await requestSiteAccess();
  }

  /** Stands in while there are no settings; the form is not shown then, so these values never appear. */
  const EMPTY: ExtensionSettings = {
    capture_enabled: false,
    ask_before_download: false,
    show_media_button: false,
    ignored_sites: [],
    capture_extensions: [],
    min_capture_size: 0,
    default_queue: 0,
  };

  const snapshot = $derived(model.snapshot);
  const notices = $derived(snapshot?.notices ?? []);

  let mainArea = $state<HTMLElement | null>(null);
  let noticeList = $state<HTMLUListElement | null>(null);
  /** A dismissed notice and its place, until the list without it arrives and focus can move. */
  let dismissed = $state<{ id: string; index: number } | null>(null);

  function dismissNotice(id: string, index: number): void {
    dismissed = { id, index };
    model.dismissNotice(id);
  }

  // Focus would otherwise fall to the page with the removed button: it goes to the notice now
  // in that place (or the last one), or to the main area when none is left. Focus the person
  // moved elsewhere in the meantime stays there.
  $effect(() => {
    if (dismissed === null) return;
    const { id, index } = dismissed;
    if (notices.some((n) => n.id === id)) return;
    dismissed = null;
    const active = document.activeElement;
    if (active !== null && active !== document.body) return;
    const buttons = noticeList?.querySelectorAll('button') ?? [];
    (buttons[Math.min(index, buttons.length - 1)] ?? mainArea)?.focus();
  });
  const settings = $derived(model.settings ?? EMPTY);
  const queues = $derived(snapshot?.queues ?? []);
  const editable = $derived(model.editable);
  const status = $derived(snapshot?.status ?? { state: 'connecting' as const });
  const problem = $derived(status.state === 'unavailable' ? status.problem : null);
  const problemText = $derived(problem === null ? null : describeProblem(problem.kind));

  const cachedNote = $derived.by(() => {
    if (!snapshot?.settingsFromCache) return null;
    const when =
      snapshot.cachedAt === null ? '' : ` from ${new Date(snapshot.cachedAt).toLocaleString()}`;
    return `Showing your last saved settings${when}. They can be changed again once Wirecatch is connected.`;
  });

  /** With nothing to show (never connected, no cache), say so instead of showing made-up values. */
  const showSettings = $derived(model.settings !== null);
</script>

<svelte:head><title>{PRODUCT_NAME}</title></svelte:head>

<div class="popup">
  <PopupHeader {status} />

  <main aria-busy={snapshot === null} tabindex="-1" bind:this={mainArea}>
    {#if problem !== null && problemText !== null}
      {@const closed = problem.kind === 'app_closed'}
      <section
        class="panel {closed ? 'panel-info' : 'panel-warning'}"
        aria-labelledby="problem-title"
      >
        <h2 id="problem-title">{problemText.title}</h2>
        <p>{problemText.fix}</p>
        {#if problem.detail}
          <p class="detail">Details: {problem.detail}</p>
        {/if}
        <button
          type="button"
          class="button"
          onclick={() => (closed ? void model.openApp() : model.reconnect())}
        >
          {closed ? 'Open Wirecatch' : 'Try again'}
        </button>
      </section>
    {/if}

    {#if !hasSiteAccess}
      <section class="panel panel-warning" aria-labelledby="access-title">
        <h2 id="access-title">Wirecatch cannot read website cookies.</h2>
        <p>Downloads stay in the browser until you allow access to websites.</p>
        <button type="button" class="button" onclick={allowAccess}>Allow access</button>
      </section>
    {/if}

    {#if notices.length > 0}
      <ul class="notices" aria-label="Messages" bind:this={noticeList}>
        {#each notices as notice, index (notice.id)}
          <li class="panel panel-{notice.tone}">
            <p id="notice-{notice.id}">{notice.message}</p>
            <button
              type="button"
              class="link-button"
              aria-describedby="notice-{notice.id}"
              onclick={() => dismissNotice(notice.id, index)}>Dismiss</button
            >
          </li>
        {/each}
      </ul>
    {/if}

    {#if cachedNote !== null}
      <p class="note">{cachedNote}</p>
    {/if}

    <MediaSection
      view={model.media}
      statuses={model.mediaStatus}
      buttonOn={model.settings?.show_media_button ?? null}
      ondownload={(key: string) => model.downloadMedia(key)}
      shortcut={mediaShortcut}
    />

    {#if !showSettings}
      <p class="note">
        {status.state === 'connecting'
          ? 'Connecting to Wirecatch. Your settings show here once it is connected.'
          : 'Your settings show here once Wirecatch is connected.'}
      </p>
    {:else}
      <SettingsForm {model} {settings} {queues} {editable} {currentUrl} />
    {/if}
  </main>

  <footer class="footer">
    <div class="actions">
      <button
        type="button"
        class="button"
        disabled={!model.connected}
        onclick={() => model.pauseAll()}
      >
        Pause all
      </button>
      <button
        type="button"
        class="button"
        disabled={!model.connected}
        onclick={() => model.resumeAll()}
      >
        Resume all
      </button>
    </div>
    <p class="feedback feedback-{model.feedback?.tone ?? 'none'}" role="status">
      {model.feedback?.text ?? ''}
    </p>
  </footer>
</div>

<style>
  /* Chromium and Firefox cap popups at 600px: only the middle scrolls, so the status and the
     Pause all and Resume all buttons are always in view. */
  .popup {
    display: flex;
    flex-direction: column;
    max-height: 600px;
  }

  .panel {
    margin: 12px 0 0;
    padding: 10px 12px;
    border-radius: 8px;
    border: 1px solid var(--warn-border);
    background: var(--warn-bg);
  }

  .notices {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .panel-error {
    border-color: var(--error-border);
    background: var(--error-bg);
  }

  /* A state to know about, not a problem: Wirecatch is closed. */
  .panel-info {
    border-color: var(--border);
    background: var(--surface);
  }

  .panel h2 {
    margin: 0 0 4px;
    font-size: 13px;
  }

  .panel p {
    margin: 0 0 8px;
  }

  .detail {
    color: var(--muted);
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .note {
    margin: 12px 0 0;
    color: var(--muted);
  }

  main {
    /* Contains the visually hidden labels, which would otherwise stretch the page. */
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    /* Room under the last line, so text at the end never sits against the footer's border. */
    padding: 0 16px 12px;
    /* A shadow at the top or bottom edge while more content is hidden there, so a line cut
       by the edge reads as "scroll for more" rather than as cut-off text. The two covers
       scroll with the content (local) and hide each shadow once that end is reached. */
    background:
      linear-gradient(var(--bg) 30%, transparent) center top / 100% 24px no-repeat local,
      linear-gradient(transparent, var(--bg) 70%) center bottom / 100% 24px no-repeat local,
      radial-gradient(farthest-side at 50% 0, var(--scroll-shadow), transparent) center top / 100%
        10px no-repeat scroll,
      radial-gradient(farthest-side at 50% 100%, var(--scroll-shadow), transparent) center bottom /
        100% 10px no-repeat scroll,
      var(--bg);
  }

  .footer {
    flex: none;
    padding: 10px 16px 12px;
    border-top: 1px solid var(--border);
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  .actions .button {
    flex: 1;
  }

  .feedback {
    min-height: 1.4em;
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--muted);
  }

  .feedback-error {
    color: var(--error-text);
  }
</style>
