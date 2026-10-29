<script lang="ts">
  // Saving settings to a file, or loading them from one: what goes in the file, then for an import what it would change before anything is applied, then what it did.
  import Check from '~icons/lucide/check';
  import CircleCheck from '~icons/lucide/circle-check';
  import Minus from '~icons/lucide/minus';
  import { Button, Callout, Dialog, Icon } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';
  import AddonCard from './AddonCard.svelte';
  import FolderList, { type FolderRow } from './FolderList.svelte';

  let { mode }: { mode: 'export' | 'import' } = $props();
  const app = getApp();

  let done = $state<string | null>(null);
  let busy = $state(false);

  const included = [
    'All settings, including the browser extension settings',
    'Queues with their schedules and speed limits',
    'Folders by file type',
    'Sites to ignore',
    'Which add-ons are installed (on import, Wirecatch asks before installing them)',
  ];
  const excluded = ['Download history', 'Saved site passwords'];

  // What the import changed, as the core reported it.
  const summary = $derived(app.transfer.lastImport);
  // Add-ons the file lists that are not installed here. The core downloads nothing on import;
  // the person decides here.
  const offered = $derived(
    (summary?.addons_to_offer ?? []).flatMap(
      (id) => app.addons.list?.find((a) => a.id === id) ?? [],
    ),
  );
  let doneButton = $state<HTMLButtonElement | null>(null);
  // The button that started the import is gone once it is done; Done takes the focus.
  $effect(() => {
    if (done) doneButton?.focus();
  });
  const moved = $derived(summary?.folders.filter((f) => f.used !== f.requested) ?? []);

  const movedRows = $derived(
    moved.map((f): FolderRow => ({
      setting: f.setting,
      resolved: f.resolved,
      note: f.reason ?? `The file asked for ${f.requested}, which is not here.`,
      flagged: false,
    })),
  );
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
  const preview = $derived(
    mode === 'import' ? (app.transfer.importPreview?.preview ?? null) : null,
  );
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
    (preview?.addons_to_offer ?? []).flatMap(
      (id) => app.addons.list?.find((a) => a.id === id) ?? [],
    ),
  );
  const previewRows = $derived(
    (preview?.folders ?? []).map((f): FolderRow => ({
      setting: f.setting,
      resolved: f.resolved,
      note: f.reason
        ? `${f.reason} Downloads go to the folder above instead.`
        : f.outside_user_folders
          ? 'This folder is outside your own folders. Check that you want downloads there.'
          : null,
      flagged: f.reason !== null || f.outside_user_folders,
    })),
  );
  let importButton = $state<HTMLButtonElement | null>(null);
  // The button that chose the file is gone once the preview shows; Import takes the focus.
  $effect(() => {
    if (preview && !done && !busy) importButton?.focus();
  });

  async function run() {
    busy = true;
    if (mode === 'export') done = await app.transfer.exportSettings();
    else if (preview) done = await app.transfer.importSettings();
    else await app.transfer.previewImport();
    busy = false;
  }
</script>

<Dialog
  open
  size="lg"
  title={mode === 'export' ? 'Export settings' : 'Import settings'}
  description={mode === 'export'
    ? 'Saves your setup to one file, to load on another Windows or Linux computer. Folders like Downloads are matched to that computer.'
    : 'Loads a file made with Export settings. You see what it changes before anything is replaced; your downloads stay as they are.'}
  onclose={() => app.closeDialog()}
>
  <div class="flex flex-col gap-4 pb-1">
    {#if done}
      <p class="flex items-start gap-2 text-ui break-all text-ink" role="status">
        <Icon icon={CircleCheck} size="md" class="mt-0.5 shrink-0 text-success" />
        <span>
          {mode === 'export' ? 'Settings saved to' : 'Settings loaded from'}
          <strong class="font-medium">{done}</strong>
        </span>
      </p>
      {#if mode === 'import' && summary}
        {#if queuesLine}
          <p class="text-ui text-ink-muted">{queuesLine}</p>
        {/if}
        {#each offered as addon (addon.id)}
          <section class="flex flex-col gap-2 rounded-md border border-line p-3">
            <h3 class="text-ui font-medium text-ink">{addon.name}</h3>
            <p class="text-small text-ink-muted" id="offer-{addon.id}">
              The file lists the {addon.name.toLowerCase()}, which is not installed here. Nothing
              was downloaded. Install it now, or later from Settings > Add-ons.
            </p>
            <AddonCard {addon} align="start" describedby="offer-{addon.id}" />
          </section>
        {/each}
        {#if moved.length > 0}
          <section class="flex flex-col gap-1.5">
            <h3 class="text-small font-semibold text-ink-muted">
              Folders adjusted for this computer
            </h3>
            <FolderList rows={movedRows} />
          </section>
        {/if}
      {/if}
    {:else if preview}
      <Callout tone="info"
        >Nothing has changed yet. Importing this file makes the changes below.</Callout
      >
      <section class="flex flex-col gap-1.5">
        <h3 class="text-small font-semibold text-ink-muted">Folders</h3>
        <FolderList rows={previewRows} />
      </section>
      {#if previewQueues}
        <p class="text-ui text-ink-muted">{previewQueues}</p>
      {/if}
      {#each previewAddons as addon (addon.id)}
        <p class="text-ui text-ink-muted">
          The file lists the {addon.name.toLowerCase()}, which is not installed here. {app.name} offers
          to install it after the import; nothing is downloaded without your approval.
        </p>
      {/each}
    {:else}
      <div class="grid gap-4 sm:grid-cols-2">
        <section class="flex flex-col gap-1.5">
          <h3 class="text-small font-semibold text-ink-muted">
            {mode === 'export' ? 'In the file' : 'Replaced from the file'}
          </h3>
          <ul class="flex flex-col gap-1.5">
            {#each included as item (item)}
              <li class="flex items-start gap-2 text-ui text-ink">
                <Icon icon={Check} size="sm" class="mt-0.5 shrink-0 text-accent" />
                {item}
              </li>
            {/each}
          </ul>
        </section>
        <section class="flex flex-col gap-1.5">
          <h3 class="text-small font-semibold text-ink-muted">Not included</h3>
          <ul class="flex flex-col gap-1.5">
            {#each excluded as item (item)}
              <li class="flex items-start gap-2 text-ui text-ink-muted">
                <Icon icon={Minus} size="sm" class="mt-0.5 shrink-0 text-ink-subtle" />
                {item}
              </li>
            {/each}
          </ul>
        </section>
      </div>
    {/if}
  </div>

  {#snippet footer()}
    {#if done}
      <Button variant="primary" bind:ref={doneButton} onclick={() => app.closeDialog()}>Done</Button
      >
    {:else if preview}
      <Button onclick={() => app.closeDialog()}>Cancel</Button>
      <Button variant="primary" bind:ref={importButton} loading={busy} onclick={run}
        >Replace my settings</Button
      >
    {:else}
      <Button onclick={() => app.closeDialog()}>Cancel</Button>
      <Button variant="primary" data-autofocus loading={busy} onclick={run}>
        {mode === 'export' ? 'Choose where to save' : 'Choose a settings file'}
      </Button>
    {/if}
  {/snippet}
</Dialog>
