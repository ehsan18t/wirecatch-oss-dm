<script lang="ts">
  import type { ExtensionSettings, QueueSummary } from '@wirecatch/protocol';
  import { SIZE_UNITS, splitSize, toBytes, type SizeUnit } from '@wirecatch/units';
  import { formatFileTypes, parseFileTypes } from '../lib/file-types';
  import type { PopupModel } from '../lib/popup/model.svelte';
  import { matchingSite, siteForPage } from '../lib/sites';
  import IgnoredSites from './IgnoredSites.svelte';
  import SwitchRow from './SwitchRow.svelte';

  interface Props {
    model: PopupModel;
    settings: ExtensionSettings;
    queues: QueueSummary[];
    /** False while the settings shown are the cached copy, which cannot be changed. */
    editable: boolean;
    /** Address of the active tab, for "Ignore this site". */
    currentUrl: string | null;
  }

  let { model, settings, queues, editable, currentUrl }: Props = $props();

  /** The default queue was deleted in the app (or renamed away): say so instead of quietly showing another. */
  const queueMissing = $derived(
    queues.length > 0 && !queues.some((q) => q.id === settings.default_queue),
  );

  const pageSite = $derived(siteForPage(currentUrl));
  const pageIgnoredBy = $derived(
    currentUrl === null ? null : matchingSite(currentUrl, settings.ignored_sites),
  );

  const size = $derived(splitSize(settings.min_capture_size));
  let sizeError = $state<string | null>(null);

  function toggleSite(): void {
    // Add and remove one site rather than replacing the list, so an edit made at the
    // same time in another browser is not overwritten.
    if (pageIgnoredBy !== null) {
      void model.update({ remove_ignored_sites: [pageIgnoredBy] });
    } else if (pageSite !== null) {
      void model.update({ add_ignored_sites: [pageSite] });
    }
  }

  /** What the person is typing in the file types box; kept while it has focus so incoming changes do not overwrite it. */
  let typesDraft = $state<string | null>(null);

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
    label="Show a window before each download"
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
      aria-describedby={queueMissing ? 'default-queue-missing' : 'default-queue-help'}
      aria-invalid={queueMissing}
      value={settings.default_queue}
      onchange={(event) => {
        const chosen = queues.find((queue) => String(queue.id) === event.currentTarget.value);
        if (chosen) void model.update({ default_queue: chosen.id });
      }}
    >
      {#if queueMissing}
        <option value={settings.default_queue}>Missing queue (choose another)</option>
      {/if}
      {#each queues as queue (queue.id)}
        <option value={queue.id}>{queue.name}{queue.is_default ? ' (default)' : ''}</option>
      {:else}
        <option value={settings.default_queue}>Default queue</option>
      {/each}
    </select>
    {#if queueMissing}
      <p id="default-queue-missing" class="error-text">
        This queue no longer exists in Wirecatch. Choose another one.
      </p>
    {/if}
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
      value={typesDraft ?? formatFileTypes(settings.capture_extensions)}
      onfocus={(event) => (typesDraft = event.currentTarget.value)}
      oninput={(event) => (typesDraft = event.currentTarget.value)}
      onblur={() => (typesDraft = null)}
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
      <select
        bind:this={unitSelect}
        id="min-size-unit"
        value={size.unit}
        onchange={commitSizeFields}
      >
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
    onadd={(site: string) => model.update({ add_ignored_sites: [site] })}
    onremove={(site: string) => model.update({ remove_ignored_sites: [site] })}
  />
</fieldset>

<style>
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
</style>
