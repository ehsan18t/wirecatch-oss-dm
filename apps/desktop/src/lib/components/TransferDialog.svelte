<script lang="ts">
  import Check from '@lucide/svelte/icons/check';
  import X from '@lucide/svelte/icons/x';
  import { getApp } from '$lib/state/app.svelte';
  import AddonCard from './AddonCard.svelte';
  import Dialog from './Dialog.svelte';

  let { mode }: { mode: 'export' | 'import' } = $props();
  const app = getApp();

  let done = $state<string | null>(null);
  let busy = $state(false);

  const included = [
    'All settings, including the browser extension settings',
    'Queues with their schedules and speed limits',
    'Folders by file type',
    'Sites to leave to the browser',
    'Which add-ons are installed (on import, Wirecatch asks before installing them)',
  ];
  const excluded = ['Download history', 'Saved site passwords'];

  // What the import changed, as the core reported it.
  const summary = $derived(app.lastImport);
  // Add-ons the file lists that are not installed here. The core downloads nothing on import;
  // the person decides here.
  const offered = $derived(
    (summary?.addons_to_offer ?? []).flatMap((id) => app.addons?.find((a) => a.id === id) ?? []),
  );
  let doneButton = $state<HTMLButtonElement>();
  // The button that started the import is gone once it is done; Done takes the focus.
  $effect(() => {
    if (done) doneButton?.focus();
  });
  const moved = $derived(summary?.folders.filter((f) => f.used !== f.requested) ?? []);
  const queuesLine = $derived.by(() => {
    if (!summary) return '';
    const plural = (n: number) => (n === 1 ? '1 queue' : `${n} queues`);
    const parts = [
      summary.queues_added > 0 ? `added ${plural(summary.queues_added)}` : '',
      summary.queues_updated > 0 ? `updated ${plural(summary.queues_updated)}` : '',
    ].filter(Boolean);
    if (parts.length === 0) return '';
    const text = parts.join(' and ');
    return `${text.charAt(0).toUpperCase()}${text.slice(1)}.`;
  });

  // What importing the chosen file would change, shown before anything is applied.
  const preview = $derived(mode === 'import' ? (app.importPreview?.preview ?? null) : null);
  const previewQueues = $derived.by(() => {
    if (!preview) return '';
    const names = (list: string[]) => list.join(', ');
    return [
      preview.queues_added.length > 0 ? `Adds queues: ${names(preview.queues_added)}.` : '',
      preview.queues_updated.length > 0 ? `Changes queues: ${names(preview.queues_updated)}.` : '',
    ]
      .filter(Boolean)
      .join(' ');
  });
  const previewAddons = $derived(
    (preview?.addons_to_offer ?? []).flatMap((id) => app.addons?.find((a) => a.id === id) ?? []),
  );
  let importButton = $state<HTMLButtonElement>();
  // The button that chose the file is gone once the preview shows; Import takes the focus.
  $effect(() => {
    if (preview && !done && !busy) importButton?.focus();
  });

  async function run() {
    busy = true;
    if (mode === 'export') done = await app.exportSettings();
    else if (preview) done = await app.importSettings();
    else await app.previewImport();
    busy = false;
  }
</script>

<Dialog
  title={mode === 'export' ? 'Export settings' : 'Import settings'}
  description={mode === 'export'
    ? 'Saves your setup to one file. Load it on another computer, Windows or Linux, with Import settings. Folders like Downloads are matched to that computer automatically.'
    : 'Loads a file made with Export settings. It replaces your current settings and queues; your downloads stay as they are.'}
  onclose={() => app.closeDialog()}
  width={520}
>
  {#if done}
    <p class="done" role="status">
      <Check size={16} aria-hidden="true" />
      <span>
        {mode === 'export' ? 'Settings saved to' : 'Settings loaded from'}
        <strong>{done}</strong>
      </span>
    </p>
    {#if mode === 'import' && summary}
      {#if queuesLine}
        <p class="note">{queuesLine}</p>
      {/if}
      {#each offered as addon (addon.id)}
        <div class="offer">
          <h3>{addon.name}</h3>
          <p class="note" id="offer-{addon.id}">
            The file lists the {addon.name.toLowerCase()}, which is not installed here. Nothing was
            downloaded. Install it now, or later from Settings, Add-ons.
          </p>
          <AddonCard {addon} describedby="offer-{addon.id}" />
        </div>
      {/each}
      {#if moved.length > 0}
        <div class="moved">
          <h3>Folders changed to fit this computer</h3>
          <ul>
            {#each moved as folder (folder.setting)}
              <li class="folder">
                <span>{folder.setting}: <strong>{folder.resolved}</strong></span>
                <span class="was">
                  {folder.reason ?? `The file asked for ${folder.requested}, which is not here.`}
                </span>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    {/if}
  {:else if preview}
    <div class="preview">
      <p class="note lead">
        Nothing has changed yet. Importing this file replaces your settings like this:
      </p>
      <h3>Folders</h3>
      <ul>
        {#each preview.folders as folder (folder.setting)}
          <li class="folder" class:flagged={folder.reason !== null || folder.outside_user_folders}>
            <span>{folder.setting}: <strong>{folder.resolved}</strong></span>
            {#if folder.reason}
              <span class="was">{folder.reason} Downloads go to the folder above instead.</span>
            {:else if folder.outside_user_folders}
              <span class="was"
                >This folder is outside your own folders. Check that you want downloads there.</span
              >
            {/if}
          </li>
        {/each}
      </ul>
      {#if previewQueues}
        <p class="note">{previewQueues}</p>
      {/if}
      {#each previewAddons as addon (addon.id)}
        <p class="note">
          The file lists the {addon.name.toLowerCase()}, which is not installed here. Wirecatch
          offers it after the import; nothing is downloaded without you.
        </p>
      {/each}
    </div>
  {:else}
    <div class="lists">
      <div>
        <h3>{mode === 'export' ? 'Saved in the file' : 'Replaced from the file'}</h3>
        <ul>
          {#each included as item (item)}
            <li><Check size={14} aria-hidden="true" /> {item}</li>
          {/each}
        </ul>
      </div>
      <div>
        <h3>Not included</h3>
        <ul>
          {#each excluded as item (item)}
            <li class="no"><X size={14} aria-hidden="true" /> {item}</li>
          {/each}
        </ul>
      </div>
    </div>
  {/if}

  {#snippet footer()}
    {#if done}
      <button class="btn primary" bind:this={doneButton} onclick={() => app.closeDialog()}
        >Done</button
      >
    {:else if preview}
      <button class="btn" onclick={() => app.closeDialog()}>Cancel</button>
      <button class="btn primary" bind:this={importButton} disabled={busy} onclick={run}
        >Import</button
      >
    {:else}
      <button class="btn" onclick={() => app.closeDialog()}>Cancel</button>
      <button class="btn primary" data-autofocus disabled={busy} onclick={run}>
        {mode === 'export' ? 'Choose where to save' : 'Choose a settings file'}
      </button>
    {/if}
  {/snippet}
</Dialog>

<style>
  .lists {
    display: grid;
    gap: 14px;
  }

  h3 {
    margin: 0 0 6px;
    font-size: var(--text-sm);
    font-weight: 600;
  }

  ul {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  li {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  li :global(svg) {
    flex: none;
    color: var(--accent-text);
  }

  li.no :global(svg) {
    color: var(--ink-3);
  }

  .done {
    display: flex;
    gap: 8px;
    margin: 0;
    line-height: 1.5;
    overflow-wrap: anywhere;
  }

  .note {
    margin: 10px 0 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .moved {
    margin-top: 14px;
  }

  .preview {
    display: grid;
    gap: 6px;
  }

  .preview .note {
    margin: 4px 0 0;
  }

  .preview .lead {
    margin: 0 0 6px;
  }

  li.folder.flagged strong {
    color: var(--danger);
  }

  .offer {
    display: grid;
    gap: 6px;
    margin-top: 14px;
  }

  .offer h3,
  .offer .note {
    margin: 0;
  }

  /* The card sits on the right in Settings; here it follows the text. */
  .offer :global(.addon),
  .offer :global(.status) {
    justify-items: start;
    text-align: left;
  }

  li.folder {
    display: grid;
    gap: 2px;
    overflow-wrap: anywhere;
  }

  li.folder strong {
    color: var(--ink);
    font-weight: 500;
  }

  .was {
    color: var(--ink-3);
    font-size: var(--text-xs);
  }

  .done :global(svg) {
    flex: none;
    margin-top: 3px;
    color: var(--accent-text);
  }
</style>
