<script lang="ts">
  // Above the list: what it shows and how many, how it is sorted, what acts on every download, and in a queue that queue's own actions.
  import PanelRight from '~icons/lucide/panel-right';
  import Pause from '~icons/lucide/pause';
  import Pencil from '~icons/lucide/pencil';
  import Play from '~icons/lucide/play';
  import { Button, Select, type SelectOption } from '@wirecatch/ui';
  import { formatRate } from '@wirecatch/units';
  import { CATEGORY_LABEL, formatSchedule } from '$lib/format';
  import { keysOf } from '$lib/keys';
  import { getApp } from '$lib/state/app.svelte';
  import { SORT_LABEL, STATUS_GROUP_LABEL, type SortKey } from '$lib/state/view';

  const app = getApp();

  const SORTS: readonly SelectOption<SortKey>[] = keysOf(SORT_LABEL).map((key) => ({
    value: key,
    label: SORT_LABEL[key],
  }));

  const queue = $derived(app.view.kind === 'queue' ? app.queues.byId(app.view.id) : undefined);

  const title = $derived.by(() => {
    const v = app.view;
    switch (v.kind) {
      case 'all':
        return 'All downloads';
      case 'status':
        return STATUS_GROUP_LABEL[v.group];
      case 'queue':
        return queue?.name ?? 'Queue';
      case 'category':
        return CATEGORY_LABEL[v.category];
    }
  });

  /** What the queue is doing, when it runs and how fast it may go. */
  const queueLine = $derived.by(() => {
    if (!queue) return '';
    const limit =
      queue.speed_limit === null ? 'no speed limit' : `up to ${formatRate(queue.speed_limit)}`;
    return `${app.queues.status(queue)}. ${formatSchedule(queue.schedule)}, ${limit}.`;
  });

  const count = $derived(app.visible.length);
  const search = $derived(app.search.trim());
  /** How many the list shows, and what they match: one string, so no markup can eat a space. */
  const countLine = $derived(
    `${count === 1 ? '1 download' : `${count.toLocaleString('en-US')} downloads`}${search ? ` matching "${search}"` : ''}`,
  );
  /** Nothing downloaded yet: the empty list explains itself, and there is nothing to sort, pause or show. */
  const firstRun = $derived(app.counts.all === 0);
  // Nothing to pause or resume: the button says so rather than doing nothing.
  const canPauseAll = $derived(app.counts.status.active + app.counts.status.waiting > 0);
  const canResumeAll = $derived(app.counts.status.paused > 0);
</script>

<div class="flex shrink-0 flex-col gap-3 px-5 pt-4 pb-3">
  <div class="flex min-w-0 items-center gap-4">
    <div class="flex min-w-0 flex-1 flex-col">
      <h1 class="truncate font-display text-title font-semibold tracking-display text-ink">
        {title}
      </h1>
      {#if !firstRun}
        <p class="truncate text-small text-ink-muted tabular-nums">{countLine}</p>
      {/if}
    </div>
    {#if !firstRun}
      <div class="flex shrink-0 items-center gap-1.5" role="group" aria-label="List tools">
        <Select
          value={app.sort}
          options={SORTS}
          label="Sort"
          size="sm"
          class="w-36"
          onchange={(value: SortKey) => (app.sort = value)}
        />
        <Button
          size="sm"
          variant="ghost"
          icon={Pause}
          disabled={!canPauseAll}
          tooltip={canPauseAll ? 'Pause every download' : 'Nothing is running or waiting'}
          onclick={() => app.actions.pauseAll()}>Pause all</Button
        >
        <Button
          size="sm"
          variant="ghost"
          icon={Play}
          disabled={!canResumeAll}
          tooltip={canResumeAll ? 'Resume every paused download' : 'Nothing is paused'}
          onclick={() => app.actions.resumeAll()}>Resume all</Button
        >
        <Button
          size="sm"
          variant="ghost"
          icon={PanelRight}
          aria-pressed={app.detailsOpen}
          tooltip={app.detailsOpen ? 'Hide the details' : 'Show the details of the selection'}
          onclick={() => (app.detailsOpen ? app.closeDetails() : app.openDetails())}>Details</Button
        >
      </div>
    {/if}
  </div>
  {#if queue}
    <div class="flex min-w-0 items-center gap-1" role="group" aria-label="This queue">
      <Button
        size="sm"
        variant="secondary"
        icon={Pencil}
        onclick={() => app.openDialog({ kind: 'queue', id: queue.id })}>Edit queue</Button
      >
      {#if queue.paused}
        <Button
          size="sm"
          variant="secondary"
          icon={Play}
          onclick={() => app.queues.setPaused(queue.id, false)}>Resume queue</Button
        >
      {:else}
        <Button
          size="sm"
          variant="secondary"
          icon={Pause}
          onclick={() => app.queues.setPaused(queue.id, true)}>Pause queue</Button
        >
      {/if}
      {#if app.queues.isOutsideSchedule(queue)}
        <Button
          size="sm"
          variant="secondary"
          icon={Play}
          onclick={() => app.queues.startNow(queue.id)}>Start now</Button
        >
      {/if}
      <span class="ml-2 truncate text-small text-ink-muted">{queueLine}</span>
    </div>
  {/if}
</div>
