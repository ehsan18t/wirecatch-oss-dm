<script lang="ts">
  // One download in the list: its kind, its name over its progress, its status, and how much, how fast and how long. Each row reads its own download, field by field, so a progress tick redraws only the rows whose numbers moved. While the pointer is on the row (or the keyboard, as a hint), its buttons take the place of its last figures, which move under the name, so nothing live is covered.
  import type { DownloadId } from '@wirecatch/protocol';
  import { formatSpeed } from '@wirecatch/units';
  import { Button, cn, ProgressBar, textIfCut, tooltip } from '@wirecatch/ui';
  import {
    FIX_LABEL,
    folderOf,
    formatEta,
    formatPercent,
    formatSize,
    hostOf,
    statusShort,
  } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import CategoryIcon from '../CategoryIcon.svelte';
  import { rowProgress, STATUS_TEXT } from '../status';
  import { CELL, FOLDED } from './columns';
  import RowActions from './RowActions.svelte';

  let { id, index }: { id: DownloadId; index: number } = $props();

  const app = getApp();
  const row = $derived(app.downloads.get(id));
  const selected = $derived(app.selection.has(id));
  const active = $derived(app.selection.active === id);
  const progress = $derived(row && row.status !== 'completed' ? rowProgress(row) : null);
  const percent = $derived(row ? formatPercent(row.done, row.total) : '');
  const size = $derived(row ? formatSize(row.done, row.total, row.status) : '');
  const speed = $derived(row ? formatSpeed(row.speed) : '');
  const eta = $derived(row ? formatEta(row.eta_secs) : '');
  /** Under a finished file's name, the folder it is in; under one with no progress to show, the site it comes from. */
  const where = $derived(
    row ? (row.status === 'completed' ? folderOf(row.path) : hostOf(row.url)) : '',
  );
  /** The figures the row's buttons cover give way to them, under the pointer and on the keyboard row. */
  const covered = $derived(
    cn('transition-opacity group-hover:opacity-0', active && 'group-focus-visible/list:opacity-0'),
  );
  /** In a list too narrow for the number columns the buttons cover the status, which gives way to them as well. */
  const statusCovered = $derived(
    cn(
      'transition-opacity @max-list-size/grid:group-hover:opacity-0',
      active && '@max-list-size/grid:group-focus-visible/list:opacity-0',
    ),
  );
  /** And show under the name instead. */
  const uncovered = $derived(cn('group-hover:inline', active && 'group-focus-visible/list:inline'));
</script>

{#if row}
  <div
    id="row-{id}"
    role="row"
    aria-selected={selected}
    aria-rowindex={index + 2}
    data-id={id}
    draggable="true"
    class={cn(
      'group relative flex h-row items-center gap-3 rounded-md px-3 text-ui',
      selected ? 'bg-accent-soft' : 'hover:bg-wash',
      active &&
        'group-focus-visible/list:before:absolute group-focus-visible/list:before:inset-y-3 group-focus-visible/list:before:left-0 group-focus-visible/list:before:w-0.75 group-focus-visible/list:before:rounded-full group-focus-visible/list:before:bg-accent forced-colors:group-focus-visible/list:before:bg-sys-highlight',
      active && !selected && 'group-focus-visible/list:bg-wash',
    )}
  >
    <span role="gridcell" class={CELL.type}><CategoryIcon category={row.category} /></span>
    <span role="gridcell" class={cn(CELL.name, 'flex flex-col justify-center gap-1.5')}>
      <span
        class="truncate font-medium text-ink"
        data-menu-anchor
        {@attach tooltip(() => textIfCut(row.file_name))}>{row.file_name}</span
      >
      {#if row.status === 'failed' && row.error}
        {@const fix = row.error.fix}
        {@const message = row.error.message}
        <span class="flex min-w-0 items-center gap-2 text-small">
          <span class="truncate text-danger" {@attach tooltip(() => textIfCut(message))}
            >{message}</span
          >
          {#if fix}
            <!-- Reached by the pointer here; from the keyboard, Enter on the row runs the same fix, which also leads its context menu. -->
            <Button
              size="sm"
              variant="ghost"
              class="h-control-xs shrink-0 px-2 text-accent"
              tabindex={-1}
              onclick={() => app.actions.fix(row.id, fix)}>{FIX_LABEL[fix]}</Button
            >
          {/if}
        </span>
      {:else if progress}
        <span class="flex h-4 min-w-0 items-center gap-2">
          <ProgressBar
            value={progress.value}
            state={progress.state}
            size="sm"
            label={row.file_name}
            valueText={progress.valueText}
            class="max-w-prose"
          />
          <span class="flex shrink-0 gap-2 text-small text-ink-muted tabular-nums">
            <span class="w-9">{percent}</span>
            <span class={cn(FOLDED.size, uncovered)}>{size}</span>
            {#if speed}<span class={cn(FOLDED.speed, uncovered)}>{speed}</span>{/if}
            {#if eta}<span class={cn(FOLDED.eta, uncovered)}>{eta} left</span>{/if}
          </span>
        </span>
      {:else}
        <span class="flex min-w-0 gap-2 text-small text-ink-subtle">
          <span class="truncate" {@attach tooltip(() => textIfCut(where))}>{where}</span>
          <span class={cn('shrink-0 tabular-nums', FOLDED.size, uncovered)}>{size}</span>
        </span>
      {/if}
    </span>
    <span role="gridcell" class={cn(CELL.status, STATUS_TEXT[row.status], statusCovered)}
      >{statusShort(row)}</span
    >
    <span role="gridcell" class={cn(CELL.size, covered)}>{size}</span>
    <span
      role="gridcell"
      class={cn(CELL.speed, covered)}
      {@attach tooltip(() =>
        row.connections > 0 ? `${String(row.connections)} connections` : null,
      )}>{speed}</span
    >
    <span role="gridcell" class={cn(CELL.eta, covered)}>{eta}</span>
    <span role="gridcell" class={CELL.actions}><RowActions {row} {active} /></span>
  </div>
{/if}
