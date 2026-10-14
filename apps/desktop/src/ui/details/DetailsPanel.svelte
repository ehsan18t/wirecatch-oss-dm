<script lang="ts">
  // The details of the selection, in a panel from the right: a column beside the list in wide content, where the list stays usable and choosing another row shows that one's details; over the list with a veil in narrow content. Escape closes it (before Escape clears the selection), as do its close button and the Details button.
  import type { QueueId } from '@wirecatch/protocol';
  import { formatBytes } from '@wirecatch/units';
  import { Badge, Kbd, Select, SlideOver } from '@wirecatch/ui';
  import { statusShort } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import { STATUS_TONE } from '../status';
  import DetailActions from './DetailActions.svelte';
  import OneDetails from './OneDetails.svelte';

  /** Beside the list rather than over it with a veil. */
  let { docked }: { docked: boolean } = $props();

  const app = getApp();

  const rows = $derived(app.selection.rows);
  const one = $derived(rows.length === 1 ? rows[0] : undefined);
  const open = $derived(app.detailsOpen && app.screen === 'downloads');
  const title = $derived(
    one ? one.file_name : rows.length > 1 ? `${String(rows.length)} downloads selected` : 'Details',
  );
  const description = $derived.by(() => {
    if (one) return undefined;
    if (rows.length === 0) return 'Nothing is selected';
    const total = rows.reduce((sum, r) => sum + (r.total ?? 0), 0);
    return `${formatBytes(total)} in total`;
  });
  const sharedQueue = $derived(
    rows.length > 0 && rows.every((r) => r.queue === rows[0]?.queue)
      ? (rows[0]?.queue ?? null)
      : null,
  );
  const queues = $derived(app.queues.list.map((q) => ({ value: q.id, label: q.name })));
</script>

{#snippet actions()}
  <DetailActions {rows} />
{/snippet}

<SlideOver
  {open}
  {docked}
  modal={!docked}
  {title}
  {description}
  footer={rows.length > 0 ? actions : undefined}
  onclose={() => app.closeDetails()}
>
  {#snippet meta()}
    {#if one}
      <Badge tone={STATUS_TONE[one.status]} size="sm" dot>{statusShort(one)}</Badge>
    {/if}
  {/snippet}
  {#if one}
    <OneDetails row={one} />
  {:else if rows.length > 1}
    <div class="flex flex-col gap-4">
      <div class="flex items-center gap-3">
        <span class="w-28 shrink-0 text-small text-ink-muted">Move to queue</span>
        <Select
          value={sharedQueue}
          options={queues}
          label="Move to queue"
          placeholder="Choose a queue"
          size="sm"
          class="min-w-0 flex-1"
          onchange={(queue: QueueId) => app.actions.moveToQueue(app.selection.list, queue)}
        />
      </div>
      <p class="text-small text-ink-muted">
        Tip: drag the selection onto a queue in the sidebar to move it there.
      </p>
    </div>
  {:else}
    <div class="flex flex-col gap-3 text-ui text-ink-muted">
      <p>Select a download to see where it is saved, its link and what you can do with it.</p>
      <p class="flex flex-wrap items-center gap-1">
        Hold <Kbd keys={['Ctrl']} /> or <Kbd keys={['Shift']} /> to select several.
      </p>
    </div>
  {/if}
</SlideOver>
