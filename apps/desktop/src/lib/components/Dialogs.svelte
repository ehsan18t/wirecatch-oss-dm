<script lang="ts">
  // The open dialog. Each kind of dialog has its view in one table, which must name every kind,
  // so a new kind cannot open the wrong dialog or none.
  import type { Snippet } from 'svelte';
  import { getApp, type Dialog } from '$lib/state/app.svelte';
  import AddDialog from '../../ui/add/AddDialog.svelte';
  import CancelDialog from './CancelDialog.svelte';
  import RemoveAddonDialog from './RemoveAddonDialog.svelte';
  import QueueDialog from './QueueDialog.svelte';
  import RefreshDialog from './RefreshDialog.svelte';
  import RemoveDialog from './RemoveDialog.svelte';
  import ShortcutsDialog from './ShortcutsDialog.svelte';
  import TransferDialog from './TransferDialog.svelte';

  const app = getApp();

  type Kind = Dialog['kind'];
  type DialogOf<K extends Kind> = Extract<Dialog, { kind: K }>;

  const VIEWS: { [K in Kind]: Snippet<[DialogOf<K>]> } = {
    add: addView,
    remove: removeView,
    cancel: cancelView,
    queue: queueView,
    refresh: refreshView,
    export: transferView,
    import: transferView,
    shortcuts: shortcutsView,
    'remove-addon': removeAddonView,
  };

  /** The view for a dialog of kind `K`. */
  function viewOf<K extends Kind>(d: DialogOf<K> & { kind: K }): Snippet<[DialogOf<K>]> {
    return VIEWS[d.kind];
  }
</script>

{#if app.dialog}
  {@const d = app.dialog}
  {#key d}
    {@render viewOf(d)(d)}
  {/key}
{/if}

{#snippet addView(_: DialogOf<'add'>)}
  {#if app.addDraft}<AddDialog draft={app.addDraft} />{/if}
{/snippet}

{#snippet removeView(d: DialogOf<'remove'>)}
  <RemoveDialog ids={d.ids} />
{/snippet}

{#snippet cancelView(d: DialogOf<'cancel'>)}
  <CancelDialog ids={d.ids} />
{/snippet}

{#snippet queueView(d: DialogOf<'queue'>)}
  <QueueDialog id={d.id} deleting={d.deleting ?? false} />
{/snippet}

{#snippet refreshView(d: DialogOf<'refresh'>)}
  <RefreshDialog id={d.id} />
{/snippet}

{#snippet transferView(d: DialogOf<'export' | 'import'>)}
  <TransferDialog mode={d.kind} />
{/snippet}

{#snippet shortcutsView(_: DialogOf<'shortcuts'>)}
  <ShortcutsDialog />
{/snippet}

{#snippet removeAddonView(d: DialogOf<'remove-addon'>)}
  <RemoveAddonDialog id={d.id} />
{/snippet}
