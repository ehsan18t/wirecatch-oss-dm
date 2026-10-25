<script lang="ts">
  // Above the list: what it shows, one line on what is happening right now, and on one row the actions for what is on screen: a queue's own (start now, pause, edit) in a queue, else Pause all or Resume all where one applies. The total speed is the title bar's to say; problems are the sidebar's and the Needs attention group's.
  import Pause from '~icons/lucide/pause';
  import Pencil from '~icons/lucide/pencil';
  import Play from '~icons/lucide/play';
  import Plus from '~icons/lucide/plus';
  import { Button } from '@wirecatch/ui';
  import { CATEGORY_LABEL, formatEta, formatSchedule } from '$lib/format';
  import { queueLimitFacts } from '$lib/limits';
  import { ariaShortcut, shortcutLabel } from '$lib/keymap';
  import { getApp } from '$lib/state/app.svelte';
  import { groupOf, sectionIds, sectionOf, STATUS_GROUP_LABEL } from '$lib/state/view';

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
        return queue?.settings.name ?? 'Queue';
      case 'category':
        return CATEGORY_LABEL[v.category];
    }
  });

  /** When a queue runs and how fast it may go, said once: the sidebar says whether it runs now. */
  const queueFacts = $derived.by(() => {
    if (!queue) return [];
    return [
      queue.status.paused ? 'Paused' : formatSchedule(queue.settings.schedule),
      ...queueLimitFacts(queue.settings),
    ].filter(Boolean);
  });

  const count = $derived(app.counts.all);
  const search = $derived(app.search.trim());
  /** The downloads of this view that download now: only these are read on each progress tick. */
  const downloading = $derived.by(() => {
    const section = sectionOf(app.layout, app.view, 'active');
    if (section) return sectionIds(app.layout, section);
    // A queue's own view is one line, not split by state: its downloading ones are found by status.
    if (!queue) return [];
    return app.downloads.idsInQueue(queue.id).filter((id) => {
      const row = app.downloads.get(id);
      return row !== undefined && groupOf(row.status) === 'active';
    });
  });
  /** The most time any download here has left, in seconds; -1 when none can say. */
  function longestLeft(): number {
    let longest = -1;
    for (const id of downloading) {
      const eta = app.downloads.get(id)?.eta_secs ?? null;
      if (eta !== null) longest = Math.max(longest, eta);
    }
    return longest;
  }

  /** What is happening in this view now: how many download and about how long the longest has left, and how many are up next; else how many it holds. One string, so no markup can eat a space. */
  const summary = $derived.by(() => {
    const inView = app.layout.sections.reduce((sum, s) => sum + s.count, 0);
    const downloads = inView === 1 ? '1 download' : `${inView.toLocaleString('en-US')} downloads`;
    if (search) return `${downloads} matching “${search}”`;
    const waiting = queue
      ? (app.counts.queue.get(queue.id)?.waiting ?? 0)
      : (sectionOf(app.layout, app.view, 'waiting')?.count ?? 0);
    const next = waiting > 0 ? `${String(waiting)} up next` : '';
    if (downloading.length > 0) {
      const longest = longestLeft();
      return [
        `${String(downloading.length)} downloading`,
        longest >= 0 ? `about ${formatEta(longest)} left` : '',
        next,
      ]
        .filter(Boolean)
        .join(' · ');
    }
    return next ? `Nothing downloading · ${next}` : downloads;
  });
  const line = $derived([...queueFacts, summary].join(' · '));
  /** Nothing downloaded yet: the empty list explains itself, and there is nothing to pause or show. */
  const firstRun = $derived(count === 0);
  /** The one of Pause all and Resume all that applies, outside a queue: pausing while anything runs or waits, else resuming what is paused. Neither in a view of finished, failed or cancelled downloads. */
  const everyDownload = $derived.by((): 'pause' | 'resume' | null => {
    const v = app.view;
    if (queue || (v.kind === 'status' && !['active', 'waiting', 'paused'].includes(v.group)))
      return null;
    if (app.counts.status.active + app.counts.status.waiting > 0) return 'pause';
    return app.counts.status.paused > 0 ? 'resume' : null;
  });
</script>

<div class="flex shrink-0 items-end gap-4 px-5 pt-4 pb-3">
  <div class="flex min-w-0 flex-1 flex-col gap-0.5">
    <h1 class="truncate font-display text-title font-semibold tracking-display text-ink">
      {title}
    </h1>
    {#if !firstRun}
      <p class="truncate text-small text-ink-muted tabular-nums">{line}</p>
    {/if}
  </div>
  <div
    class="flex shrink-0 items-center gap-1"
    role="group"
    aria-label={queue ? 'This queue' : 'List tools'}
  >
    {#if queue}
      {#if app.queues.isOutsideSchedule(queue)}
        <Button size="sm" variant="ghost" icon={Play} onclick={() => app.queues.startNow(queue.id)}
          >Start now</Button
        >
      {/if}
      {#if queue.status.paused}
        <Button
          size="sm"
          variant="ghost"
          icon={Play}
          onclick={() => app.queues.setPaused(queue.id, false)}>Resume queue</Button
        >
      {:else}
        <Button
          size="sm"
          variant="ghost"
          icon={Pause}
          onclick={() => app.queues.setPaused(queue.id, true)}>Pause queue</Button
        >
      {/if}
      <Button
        size="sm"
        variant="ghost"
        icon={Pencil}
        onclick={() => app.openDialog({ kind: 'queue', id: queue.id })}>Edit queue</Button
      >
    {:else if everyDownload === 'pause'}
      <Button size="sm" variant="ghost" icon={Pause} onclick={() => app.actions.pauseAll()}
        >Pause all</Button
      >
    {:else if everyDownload === 'resume'}
      <Button size="sm" variant="ghost" icon={Play} onclick={() => app.actions.resumeAll()}
        >Resume all</Button
      >
    {/if}
    {#if !firstRun}
      <Button
        size="sm"
        variant="primary"
        icon={Plus}
        class="ml-1.5"
        aria-keyshortcuts={ariaShortcut('add')}
        tooltip="New download ({shortcutLabel('add')}, or paste a link anywhere)"
        onclick={() => app.openDialog({ kind: 'add', urls: [] })}>New download</Button
      >
    {/if}
  </div>
</div>
