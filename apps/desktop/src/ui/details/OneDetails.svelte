<script lang="ts">
  // One download's details: how far it has got, why it waits or what went wrong, and where it comes from and goes, with what can change there.
  import Copy from '~icons/lucide/copy';
  import type { DownloadSummary, FixAction, QueueId } from '@wirecatch/protocol';
  import { formatBytes, formatSpeed } from '@wirecatch/units';
  import { Button, Callout, IconButton, ProgressBar, Select } from '@wirecatch/ui';
  import {
    FIX_LABEL,
    formatDate,
    formatEta,
    formatPercent,
    formatSize,
    linkForDisplay,
    MEDIA_SOURCE_LABEL,
    statusDetail,
  } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import { canCancel, canChangeFolder, linkMayHaveExpired } from '$lib/state/row-actions';
  import CategoryIcon from '../CategoryIcon.svelte';
  import { rowProgress } from '../status';
  import ErrorNotice from './ErrorNotice.svelte';
  import Fact from './Fact.svelte';

  let { row }: { row: DownloadSummary } = $props();

  const app = getApp();

  const percent = $derived(formatPercent(row.done, row.total));
  const progress = $derived(rowProgress(row));
  const queueName = $derived(app.queues.byId(row.queue)?.name ?? 'Its queue');
  const detail = $derived(statusDetail(row, queueName));
  const connectionsLimit = $derived(app.settings?.connections_per_download ?? null);
  const queues = $derived(app.queues.list.map((q) => ({ value: q.id, label: q.name })));
  const readout = $derived.by(() => {
    if (row.status === 'completed') return formatBytes(row.total ?? row.done);
    if (row.speed > 0) return formatSpeed(row.speed);
    return percent || formatBytes(row.done);
  });
  const connections = $derived.by(() => {
    const limit = connectionsLimit === null ? null : `up to ${String(connectionsLimit)}`;
    if (row.connections === 0)
      return limit === null ? 'None open' : `Up to ${String(connectionsLimit)}`;
    const open = `${String(row.connections)} open now`;
    return limit === null ? open : `${open}, ${limit}`;
  });
</script>

<div class="flex flex-col gap-5">
  <section aria-label="Progress" class="flex flex-col gap-3">
    <div class="flex items-center gap-3">
      <CategoryIcon category={row.category} size="lg" />
      <div class="flex min-w-0 flex-col">
        <p class="font-display text-headline font-semibold text-ink tabular-nums">{readout}</p>
        <p class="flex flex-wrap gap-x-3 text-small text-ink-muted tabular-nums">
          <span>{formatSize(row.done, row.total, row.status)}</span>
          {#if row.status !== 'completed' && percent}<span>{percent} done</span>{/if}
          {#if row.eta_secs !== null && row.speed > 0}<span>{formatEta(row.eta_secs)} left</span
            >{/if}
        </p>
      </div>
    </div>
    {#if progress}
      <ProgressBar
        value={progress.value}
        state={progress.state}
        label="Progress of {row.file_name}"
        valueText={progress.valueText}
      />
    {/if}
  </section>

  {#if row.status === 'failed'}
    <ErrorNotice
      {row}
      onfix={(fix: FixAction) => void app.actions.fix(row.id, fix)}
      onretry={() => void app.actions.retry([row.id])}
    />
  {:else if detail}
    <Callout tone={row.waiting?.type === 'server_refused' ? 'warning' : 'info'}>{detail}</Callout>
  {/if}
  {#if row.media}
    <p class="text-small text-ink-muted">
      {row.media.quality}, {MEDIA_SOURCE_LABEL[row.media.source]}.
    </p>
  {/if}

  <dl class="flex flex-col gap-3">
    <Fact label="Queue" control>
      <Select
        value={row.queue}
        options={queues}
        label="Queue"
        size="sm"
        class="w-full"
        onchange={(queue: QueueId) => app.actions.moveToQueue([row.id], queue)}
      />
    </Fact>
    <Fact label="Connections">{connections}</Fact>
    <Fact label="Saved as">
      <span class="wrap-anywhere">{row.path}</span>
      {#if canChangeFolder(row)}
        <Button size="sm" onclick={() => app.actions.chooseFolderFor(row.id)}>Change folder</Button>
      {/if}
    </Fact>
    <Fact label="Link">
      <span class="flex w-full items-start gap-1">
        <span class="min-w-0 flex-1 wrap-anywhere">{linkForDisplay(row.url)}</span>
        <IconButton
          icon={Copy}
          label="Copy link"
          size="sm"
          onclick={() => app.actions.copyLink(row.id)}
        />
      </span>
    </Fact>
    {#if linkMayHaveExpired(row)}
      <Fact label="Link expired?" control>
        <Button size="sm" onclick={() => app.openDialog({ kind: 'refresh', id: row.id })}
          >{FIX_LABEL.refresh_link}</Button
        >
      </Fact>
    {/if}
    <Fact label="Added">{formatDate(row.added_at, app.clock.now)}</Fact>
    {#if row.completed_at !== null}
      <Fact label="Finished">{formatDate(row.completed_at, app.clock.now)}</Fact>
    {/if}
  </dl>

  {#if canCancel(row)}
    <Button
      variant="ghost"
      size="sm"
      class="self-start text-danger"
      onclick={() => app.openDialog({ kind: 'cancel', ids: [row.id] })}
      >Cancel and discard the partial file</Button
    >
  {/if}
</div>
