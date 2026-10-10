<script lang="ts">
  import type { ExtensionSettings } from '@wirecatch/protocol';
  import { formatFileTypes, parseFileTypes } from '../lib/file-types';
  import { describeProblem } from '../lib/native/status';
  import type { PopupModel } from '../lib/popup/model.svelte';
  import { SIZE_UNITS, splitSize, toBytes, type SizeUnit } from '../lib/size';
  import { matchingSite, siteForPage } from '../lib/sites';
  import IgnoredSites from './IgnoredSites.svelte';
  import SwitchRow from './SwitchRow.svelte';

  interface Props {
    model: PopupModel;
    /** Address of the active tab, for "Ignore this site". */
    currentUrl: string | null;
  }

  let { model, currentUrl }: Props = $props();

  /** Shown (disabled) until the first answer, so the layout does not jump. */
  const PLACEHOLDER: ExtensionSettings = {
    capture_enabled: true,
    ask_before_download: false,
    show_media_button: true,
    ignored_sites: [],
    capture_extensions: [],
    min_capture_size: 0,
    default_queue: 0,
  };

  const snapshot = $derived(model.snapshot);
  const settings = $derived(model.settings ?? PLACEHOLDER);
  const editable = $derived(model.editable);
  const status = $derived(snapshot?.status ?? { state: 'connecting' as const });
  const problem = $derived(status.state === 'unavailable' ? status.problem : null);
  const problemText = $derived(problem === null ? null : describeProblem(problem.kind));

  const pageSite = $derived(siteForPage(currentUrl));
  const pageIgnoredBy = $derived(
    currentUrl === null ? null : matchingSite(currentUrl, settings.ignored_sites),
  );

  const size = $derived(splitSize(settings.min_capture_size));
  let sizeError = $state<string | null>(null);

  const statusLabel = $derived.by(() => {
    switch (status.state) {
      case 'connected':
        return 'Connected';
      case 'connecting':
        return 'Connecting';
      case 'unavailable':
        return status.retryAt === null ? 'Connecting' : 'Not connected';
    }
  });

  const cachedNote = $derived.by(() => {
    if (snapshot === null || !snapshot.settingsFromCache) return null;
    const when = snapshot.cachedAt === null ? '' : ` from ${new Date(snapshot.cachedAt).toLocaleString()}`;
    return `Showing your last saved settings${when}. They can be changed again once Wirecatch is connected.`;
  });

  /** With nothing to show (never connected, no cache), say so instead of showing made-up values. */
  const showSettings = $derived(model.settings !== null || status.state !== 'unavailable');

  function toggleSite(): void {
    if (pageIgnoredBy !== null) {
      void model.update({ ignored_sites: settings.ignored_sites.filter((s) => s !== pageIgnoredBy) });
    } else if (pageSite !== null) {
      void model.update({ ignored_sites: [...settings.ignored_sites, pageSite] });
    }
  }

  function commitFileTypes(value: string): void {
    const types = parseFileTypes(value);
    if (formatFileTypes(types) !== formatFileTypes(settings.capture_extensions)) {
      void model.update({ capture_extensions: types });
    }
  }

  function commitSize(value: string, unit: SizeUnit): void {
    const bytes = toBytes(value, unit);
    if (bytes === null) {
      sizeError = 'Type a number, such as 5. Use 0 to capture every size.';
      return;
    }
    sizeError = null;
    if (bytes !== settings.min_capture_size) void model.update({ min_capture_size: bytes });
  }

  let sizeInput = $state<HTMLInputElement | null>(null);
  let unitSelect = $state<HTMLSelectElement | null>(null);

  function commitSizeFields(): void {
    if (sizeInput === null || unitSelect === null) return;
    const chosen = unitSelect.value;
    const unit = SIZE_UNITS.find((u) => u.unit === chosen)?.unit ?? 'MB';
    commitSize(sizeInput.value, unit);
  }
</script>

<div class="popup">
  <header class="header">
    <div class="brand">
      <svg class="logo" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
        <rect x="1" y="1" width="22" height="22" rx="6" />
        <path d="M12 6v9m-4-4 4 4 4-4M7 18h10" />
      </svg>
      <h1>Wirecatch</h1>
    </div>
    <p class="status status-{status.state}" aria-live="polite">
      <span class="dot" aria-hidden="true"></span>
      <span>{statusLabel}</span>
    </p>
  </header>

  <main aria-busy={snapshot === null}>
    {#if problem !== null && problemText !== null}
      <section class="panel panel-warning" aria-labelledby="problem-title">
        <h2 id="problem-title">{problemText.title}</h2>
        <p>{problemText.fix}</p>
        {#if problem.detail}
          <p class="detail">Details: {problem.detail}</p>
        {/if}
        <button type="button" class="button" onclick={() => model.reconnect()}>Try again</button>
      </section>
    {/if}

    {#if snapshot?.notice}
      <section class="panel panel-{snapshot.notice.tone}" aria-label="Message">
        <p>{snapshot.notice.message}</p>
        <button type="button" class="link-button" onclick={() => model.dismissNotice()}>Dismiss</button>
      </section>
    {/if}

    {#if cachedNote !== null}
      <p class="note">{cachedNote}</p>
    {/if}

    {#if !showSettings}
      <p class="note">Your settings show here once Wirecatch is connected.</p>
    {:else}
      <fieldset class="settings" disabled={!editable}>
        <legend class="visually-hidden">Wirecatch settings</legend>

        <SwitchRow
          id="capture-enabled"
          label="Capture downloads"
          description="Send downloads from this browser to Wirecatch."
          checked={settings.capture_enabled}
          onchange={(checked: boolean) => model.update({ capture_enabled: checked })}
        />

        <div class="row site-row">
          <div class="text">
            <span class="field-label" id="this-site-label">This site</span>
            <p class="help" id="this-site-help">
              {#if pageSite === null}
                Open a web page to ignore its site.
              {:else if pageIgnoredBy !== null}
                Downloads from {pageSite} stay in the browser.
              {:else}
                Downloads from {pageSite} go to Wirecatch.
              {/if}
            </p>
          </div>
          <button
            type="button"
            class="button"
            disabled={pageSite === null && pageIgnoredBy === null}
            aria-describedby="this-site-help"
            onclick={toggleSite}
          >
            {pageIgnoredBy !== null ? 'Stop ignoring' : 'Ignore this site'}
          </button>
        </div>

        <SwitchRow
          id="ask-before"
          label="Ask before each download"
          description="Choose where each file goes before it starts."
          checked={settings.ask_before_download}
          onchange={(checked: boolean) => model.update({ ask_before_download: checked })}
        />

        <SwitchRow
          id="media-button"
          label="Show download button on videos"
          description="Adds a button to video and audio on web pages."
          checked={settings.show_media_button}
          onchange={(checked: boolean) => model.update({ show_media_button: checked })}
        />

        <div class="field">
          <label class="field-label" for="default-queue">Default queue</label>
          <p class="help" id="default-queue-help">Downloads from the browser join this queue.</p>
          <select
            id="default-queue"
            aria-describedby="default-queue-help"
            value={settings.default_queue}
            onchange={(event) => model.update({ default_queue: Number(event.currentTarget.value) })}
          >
            {#each snapshot?.queues ?? [] as queue (queue.id)}
              <option value={queue.id}>{queue.name}{queue.is_default ? ' (main)' : ''}</option>
            {:else}
              <option value={settings.default_queue}>Default queue</option>
            {/each}
          </select>
        </div>

        <div class="field">
          <label class="field-label" for="file-types">File types to capture</label>
          <p class="help" id="file-types-help">For example: zip, exe, mp4. Leave empty for every type.</p>
          <input
            id="file-types"
            type="text"
            autocomplete="off"
            spellcheck="false"
            placeholder="Every file type"
            aria-describedby="file-types-help"
            value={formatFileTypes(settings.capture_extensions)}
            onchange={(event) => commitFileTypes(event.currentTarget.value)}
            onkeydown={(event) => {
              if (event.key === 'Enter') commitFileTypes(event.currentTarget.value);
            }}
          />
        </div>

        <div class="field">
          <label class="field-label" for="min-size">Minimum file size</label>
          <p class="help" id="min-size-help">Smaller files stay in the browser. 0 means every size.</p>
          <div class="size">
            <input
              bind:this={sizeInput}
              id="min-size"
              type="number"
              min="0"
              step="any"
              inputmode="decimal"
              aria-describedby={sizeError === null ? 'min-size-help' : 'min-size-error'}
              aria-invalid={sizeError !== null}
              value={size.value}
              onchange={commitSizeFields}
              onkeydown={(event) => {
                if (event.key === 'Enter') commitSizeFields();
              }}
            />
            <label for="min-size-unit" class="visually-hidden">Size unit</label>
            <select bind:this={unitSelect} id="min-size-unit" value={size.unit} onchange={commitSizeFields}>
              {#each SIZE_UNITS as u (u.unit)}
                <option value={u.unit}>{u.unit}</option>
              {/each}
            </select>
          </div>
          {#if sizeError !== null}
            <p id="min-size-error" class="error-text" role="alert">{sizeError}</p>
          {/if}
        </div>

        <IgnoredSites
          sites={settings.ignored_sites}
          disabled={!editable}
          onchange={(sites: string[]) => model.update({ ignored_sites: sites })}
        />
      </fieldset>
    {/if}
  </main>

  <footer class="footer">
    <div class="actions">
      <button type="button" class="button" disabled={!model.connected} onclick={() => model.pauseAll()}>
        Pause all
      </button>
      <button type="button" class="button" disabled={!model.connected} onclick={() => model.resumeAll()}>
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

  .header {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .logo {
    width: 22px;
    height: 22px;
  }

  .logo rect {
    fill: var(--accent);
  }

  .logo path {
    fill: none;
    stroke: var(--on-accent);
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  h1 {
    margin: 0;
    font-size: 15px;
    font-weight: 650;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
  }

  .status-connected .dot {
    background: var(--ok);
  }

  .status-unavailable .dot {
    background: var(--warn);
  }

  .panel {
    margin: 12px 0 0;
    padding: 10px 12px;
    border-radius: 8px;
    border: 1px solid var(--warn-border);
    background: var(--warn-bg);
  }

  .panel-error {
    border-color: var(--error-border);
    background: var(--error-bg);
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
    padding: 0 16px 4px;
  }

  .settings {
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }

  .settings > :global(* + *) {
    border-top: 1px solid var(--border);
  }

  .settings > :global(legend + *) {
    border-top: 0;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 0;
  }

  .text {
    min-width: 0;
  }

  .site-row .button {
    flex: none;
  }

  .field {
    padding: 8px 0;
  }

  .field input,
  .field select {
    width: 100%;
  }

  .size {
    display: flex;
    gap: 8px;
  }

  .size input {
    flex: 1;
  }

  .size select {
    width: auto;
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
