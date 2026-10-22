<script lang="ts">
  // One download's details, in a dialog over the list, which keeps its full width behind the veil (D47). Top to bottom: what is happening now (its problem and fix, its speed and progress, the last minute's speed), the short facts in one strip, the two long values (where it is saved, where it came from) across the whole width, then what happened to it. It keeps updating while open; the row stays selected after it closes.
  import AppWindow from '~icons/lucide/app-window';
  import CircleX from '~icons/lucide/circle-x';
  import Copy from '~icons/lucide/copy';
  import Ellipsis from '~icons/lucide/ellipsis';
  import FolderOpen from '~icons/lucide/folder-open';
  import Link from '~icons/lucide/link';
  import Pause from '~icons/lucide/pause';
  import Play from '~icons/lucide/play';
  import RotateCw from '~icons/lucide/rotate-cw';
  import SquareArrowOutUpRight from '~icons/lucide/square-arrow-out-up-right';
  import Trash from '~icons/lucide/trash-2';
  import type { DownloadSummary, QueueId } from '@wirecatch/protocol';
  import { formatBytes, formatSpeed } from '@wirecatch/units';
  import {
    Badge,
    Button,
    Callout,
    Dialog,
    Icon,
    IconButton,
    Menu,
    ProgressBar,
    Select,
    textIfCut,
    tooltip,
    type MenuEntry,
    type MenuTriggerProps,
  } from '@wirecatch/ui';
  import {
    CATEGORY_LABEL,
    FIX_LABEL,
    formatDate,
    formatEta,
    formatPercent,
    formatSize,
    hostOf,
    linkForDisplay,
    MEDIA_SOURCE_LABEL,
    statusDetail,
    STATUS_LABEL,
  } from '$lib/format';
  import { fileKind } from '$lib/file-names';
  import { getApp } from '$lib/state/app.svelte';
  import {
    applicable,
    canCancel,
    canChangeFolder,
    linkMayHaveExpired,
  } from '$lib/state/row-actions';
  import { groupOf } from '$lib/state/view';
  import CategoryIcon from '../CategoryIcon.svelte';
  import { rowProgress, STATUS_TONE } from '../status';
  import SpeedGraph from './SpeedGraph.svelte';

  const app = getApp();

  /** The short facts, each one the rest of the dialog does not already say: when it finished; its size while nothing of it is downloaded (the progress line says it after); its kind of file; when it was added. Where it stands in line is the line under its status, said once. */
  function factsOf(r: DownloadSummary): [string, string][] {
    const notStarted = r.done === 0 && r.status !== 'completed' && groupOf(r.status) !== 'active';
    return [
      ...(r.completed_at === null
        ? []
        : [['Finished', formatDate(r.completed_at, app.clock.now)] as [string, string]]),
      ...(notStarted
        ? [['Size', r.total === null ? 'Not known' : formatBytes(r.total)] as [string, string]]
        : []),
      ['Kind', fileKind(r.file_name, CATEGORY_LABEL[r.category])],
      ['Added', formatDate(r.added_at, app.clock.now)],
    ];
  }

  /** What the footer keeps behind More: its own window, the link, and cancelling. */
  function more(r: DownloadSummary): MenuEntry[] {
    return [
      {
        id: 'window',
        label: 'Open in its own window',
        icon: AppWindow,
        onselect: () => void app.showProgressWindow(r.id),
      },
      {
        id: 'copy',
        label: 'Copy link',
        icon: Copy,
        onselect: () => void app.actions.copyLink(r.id),
      },
      ...(canCancel(r)
        ? [
            {
              id: 'cancel',
              label: 'Cancel download',
              icon: CircleX,
              danger: true,
              onselect: () => app.openDialog({ kind: 'cancel', ids: [r.id] }),
            },
          ]
        : []),
    ];
  }

  const row = $derived<DownloadSummary | undefined>(
    app.details === null ? undefined : app.downloads.get(app.details),
  );
</script>

{#if row}
  {@const r = row}
  {@const group = groupOf(r.status)}
  {@const live = group === 'active'}
  {@const progress = rowProgress(r)}
  {@const percent = formatPercent(r.done, r.total)}
  {@const queueName = app.queues.byId(r.queue)?.name ?? 'Its queue'}
  {@const detail = statusDetail(r, queueName)}
  {@const can = applicable([r])}
  {@const fix = r.status === 'failed' ? r.error?.fix : undefined}
  {@const folder = r.path.endsWith(r.file_name) ? r.path.slice(0, -r.file_name.length) : ''}
  {@const facts = factsOf(r)}
  <Dialog
    open
    title={r.file_name}
    description={hostOf(r.url)}
    size="lg"
    onclose={() => app.closeDetails()}
  >
    {#snippet leading()}
      <CategoryIcon category={r.category} size="lg" />
    {/snippet}
    {#snippet meta()}
      <Badge tone={STATUS_TONE[r.status]} size="sm" dot class="shrink-0"
        >{STATUS_LABEL[r.status]}</Badge
      >
    {/snippet}
    <div class="-mx-6 -my-3 flex flex-col">
      <section aria-label="Now" class="flex flex-col gap-3 px-6 pt-3 pb-4">
        {#if r.status === 'failed' && r.error}
          <div
            class="flex items-center gap-3 rounded-md bg-danger/tint px-3 py-2.5 text-small text-danger"
            role="status"
          >
            <span class="min-w-0 flex-1">{r.error.message}</span>
            {#if fix}
              <Button size="sm" variant="primary" onclick={() => app.actions.fix(r.id, fix)}
                >{FIX_LABEL[fix]}</Button
              >
            {/if}
            {#if fix !== 'retry'}
              <Button size="sm" icon={RotateCw} onclick={() => app.actions.retry([r.id])}
                >Try again</Button
              >
            {/if}
          </div>
        {:else if detail}
          <Callout tone={r.waiting?.type === 'server_refused' ? 'warning' : 'info'}
            >{detail}</Callout
          >
        {/if}
        {#if r.done > 0 || live || r.status === 'completed'}
          <div class="flex flex-wrap items-baseline gap-x-4 gap-y-1 tabular-nums">
            {#if live && r.speed > 0}
              <strong class="font-display text-headline font-semibold text-ink"
                >{formatSpeed(r.speed)}</strong
              >
            {/if}
            <span class="text-small text-ink-muted">
              {#if r.status === 'completed'}
                {formatBytes(r.total ?? r.done)}
              {:else}
                {formatSize(r.done, r.total, r.status)}{percent && r.done > 0
                  ? ` · ${percent}`
                  : ''}
              {/if}
            </span>
            {#if live && r.eta_secs !== null && r.speed > 0}
              <span class="ml-auto text-small text-ink-muted">{formatEta(r.eta_secs)} left</span>
            {/if}
          </div>
        {/if}
        {#if progress && group !== 'waiting' && group !== 'finished'}
          <ProgressBar
            value={progress.value}
            state={progress.state}
            size="sm"
            label="Progress of {r.file_name}"
            valueText={progress.valueText}
          />
        {/if}
        {#if live}
          {#key r.id}<SpeedGraph row={r} />{/key}
        {/if}
        {#if r.media}
          <p class="text-small text-ink-muted">
            {r.media.quality}, {MEDIA_SOURCE_LABEL[r.media.source]}.
          </p>
        {/if}
      </section>

      <dl class="flex flex-wrap border-y border-line bg-layer">
        <div class="flex min-w-0 flex-1 basis-40 flex-col gap-0.5 px-6 py-2.5">
          <dt class="text-caption text-ink-subtle">Queue</dt>
          <dd class="min-w-0">
            <Select
              value={r.queue}
              options={app.queues.list.map((q) => ({ value: q.id, label: q.name }))}
              label="Queue"
              size="sm"
              class="-ml-2.5 w-full max-w-48 border-transparent bg-transparent shadow-none"
              onchange={(queue: QueueId) => app.actions.moveToQueue([r.id], queue)}
            />
          </dd>
        </div>
        {#each facts as [label, value] (label)}
          <div
            class="flex min-w-0 flex-1 basis-28 flex-col gap-0.5 border-l border-line px-4 py-2.5"
          >
            <dt class="text-caption text-ink-subtle">{label}</dt>
            <dd class="truncate text-small font-medium text-ink tabular-nums">{value}</dd>
          </div>
        {/each}
      </dl>

      <div class="flex flex-col px-6 py-1.5">
        <div class="flex items-baseline gap-3 py-2 text-small">
          <span class="flex w-32 shrink-0 items-center gap-1.5 self-center text-ink-subtle"
            ><Icon icon={FolderOpen} size="sm" />Saved to</span
          >
          <span class="min-w-0 flex-1 font-medium wrap-anywhere text-ink select-text"
            ><span class="font-normal text-ink-subtle">{folder}</span>{folder
              ? r.file_name
              : r.path}</span
          >
          {#if canChangeFolder(r)}
            <Button variant="link" size="sm" onclick={() => app.actions.chooseFolderFor(r.id)}
              >Change</Button
            >
          {/if}
        </div>
        <div class="flex items-baseline gap-3 border-t border-dashed border-line py-2 text-small">
          <span class="flex w-32 shrink-0 items-center gap-1.5 self-center text-ink-subtle"
            ><Icon icon={Link} size="sm" />From</span
          >
          <span
            class="min-w-0 flex-1 truncate font-medium text-ink select-text"
            {@attach tooltip(() => textIfCut(linkForDisplay(r.url)))}>{linkForDisplay(r.url)}</span
          >
          <Button variant="link" size="sm" onclick={() => app.actions.copyLink(r.id)}>Copy</Button>
        </div>
        {#if linkMayHaveExpired(r)}
          <div
            class="flex items-center justify-between gap-3 border-t border-dashed border-line py-2 text-small text-ink-muted"
          >
            <span>Stuck? The link may have expired.</span>
            <Button size="sm" onclick={() => app.openDialog({ kind: 'refresh', id: r.id })}
              >{FIX_LABEL.refresh_link}</Button
            >
          </div>
        {/if}
      </div>

      <section
        aria-label="Activity"
        class="flex flex-col gap-1.5 border-t border-line px-6 pt-3 pb-4 text-small"
      >
        <h3 class="text-caption font-semibold tracking-caps text-ink-subtle uppercase">Activity</h3>
        <p class="flex gap-3 text-ink-muted">
          <span class="w-32 shrink-0 whitespace-nowrap text-ink-subtle"
            >{formatDate(r.added_at, app.clock.now)}</span
          >
          <span>Added from {hostOf(r.url)}</span>
        </p>
        {#if r.completed_at !== null}
          <p class="flex gap-3 text-ink-muted">
            <span class="w-32 shrink-0 whitespace-nowrap text-ink-subtle"
              >{formatDate(r.completed_at, app.clock.now)}</span
            >
            <span>Finished</span>
          </p>
        {/if}
      </section>
    </div>
    {#snippet footer()}
      <div class="flex w-full flex-wrap items-center gap-2">
        {#if can.open}
          <Button
            variant="primary"
            icon={SquareArrowOutUpRight}
            data-autofocus
            onclick={() => app.actions.openFile(r.id)}>Open</Button
          >
        {/if}
        {#if can.pause}
          <Button icon={Pause} data-autofocus onclick={() => app.actions.pause([r.id])}
            >Pause</Button
          >
        {/if}
        {#if can.resume}
          <Button
            variant="primary"
            icon={Play}
            data-autofocus
            onclick={() => app.actions.resume([r.id])}>Resume</Button
          >
        {/if}
        {#if can.retry && r.status === 'cancelled'}
          <Button
            variant="primary"
            icon={RotateCw}
            data-autofocus
            onclick={() => app.actions.retry([r.id])}>Start over</Button
          >
        {/if}
        <Button variant="ghost" icon={FolderOpen} onclick={() => app.actions.openFolder(r.id)}
          >Show in folder</Button
        >
        <span class="ml-auto flex items-center gap-1">
          <Menu entries={more(r)} label="More for {r.file_name}" align="end">
            {#snippet trigger(props: MenuTriggerProps)}
              <IconButton {...props} icon={Ellipsis} label="More actions" />
            {/snippet}
          </Menu>
          <Button
            variant="danger-quiet"
            icon={Trash}
            onclick={() => app.openDialog({ kind: 'remove', ids: [r.id] })}>Remove</Button
          >
        </span>
      </div>
    {/snippet}
  </Dialog>
{/if}
