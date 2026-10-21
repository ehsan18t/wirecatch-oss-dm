<script lang="ts">
  // Above the list: what it shows, one line on what is happening right now, what acts on every download, and in a queue that queue's own actions. Problems are not repeated here: the sidebar and the Needs attention group already show them.
  import Pause from '~icons/lucide/pause';
  import Pencil from '~icons/lucide/pencil';
  import Play from '~icons/lucide/play';
  import Plus from '~icons/lucide/plus';
  import { Button } from '@wirecatch/ui';
  import { formatRate, formatSpeed } from '@wirecatch/units';
  import { CATEGORY_LABEL, formatEta, formatSchedule } from '$lib/format';
  import { ariaShortcut, shortcutLabel } from '$lib/keymap';
  import { getApp } from '$lib/state/app.svelte';
  import { sectionIds, sectionOf, STATUS_GROUP_LABEL } from '$lib/state/view';

  const app = getApp();

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

  const count = $derived(app.counts.all);
  const search = $derived(app.search.trim());
  /** The downloads of this view that download now: only these are read on each progress tick. */
  const downloading = $derived.by(() => {
    const section = sectionOf(app.layout, app.view, 'active');
    return section ? sectionIds(app.layout, section) : [];
  });
  /** What is happening in this view now: how many download and how fast, and about how long the longest has left; else how many it holds. One string, so no markup can eat a space. */
  const summary = $derived.by(() => {
    const inView = app.layout.sections.reduce((sum, s) => sum + s.count, 0);
    const downloads = inView === 1 ? '1 download' : `${inView.toLocaleString('en-US')} downloads`;
    if (search) return `${downloads} matching "${search}"`;
    if (downloading.length > 0) {
      let speed = 0;
      let longest = -1;
      for (const id of downloading) {
        const row = app.downloads.get(id);
        if (!row) continue;
        speed += row.speed;
        if (row.eta_secs !== null) longest = Math.max(longest, row.eta_secs);
      }
      const rate = formatSpeed(speed);
      const left = longest >= 0 ? ` · about ${formatEta(longest)} left` : '';
      return `${String(downloading.length)} downloading${rate ? ` at ${rate}` : ''}${left}`;
    }
    const waiting = sectionOf(app.layout, app.view, 'waiting')?.count ?? 0;
    if (waiting > 0) return `Nothing downloading, ${String(waiting)} up next`;
    return downloads;
  });
  /** Nothing downloaded yet: the empty list explains itself, and there is nothing to pause or show. */
  const firstRun = $derived(count === 0);
  // Nothing to pause or resume: the button says so rather than doing nothing.
  const canPauseAll = $derived(app.counts.status.active + app.counts.status.waiting > 0);
  const canResumeAll = $derived(app.counts.status.paused > 0);
</script>

<div class="flex shrink-0 flex-col gap-3 px-5 pt-4 pb-3">
  <div class="flex min-w-0 items-end gap-4">
    <div class="flex min-w-0 flex-1 flex-col gap-0.5">
      <h1 class="truncate font-display text-title font-semibold tracking-display text-ink">
        {title}
      </h1>
      {#if !firstRun}
        <p class="truncate text-small text-ink-muted tabular-nums">{summary}</p>
      {/if}
    </div>
    <div class="flex shrink-0 items-center gap-1" role="group" aria-label="List tools">
      {#if !firstRun}
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
      {/if}
      <Button
        size="sm"
        variant="soft"
        icon={Plus}
        class="ml-1.5"
        aria-keyshortcuts={ariaShortcut('add')}
        tooltip="New download ({shortcutLabel('add')}, or paste a link anywhere)"
        onclick={() => app.openDialog({ kind: 'add', urls: [] })}>New download</Button
      >
    </div>
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
