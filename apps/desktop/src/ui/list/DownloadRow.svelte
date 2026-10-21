<script lang="ts">
  // One download in the list, on the list's grid: its place in its queue, its kind of file (with a box that selects it), its name over one line about it, its progress, and how much, how fast and how long. Each row reads its own download, field by field, so a progress tick redraws only the rows whose numbers moved. A download that needs attention says what happened in that one line and offers its fix on the row.
  import Check from '~icons/lucide/check';
  import Ellipsis from '~icons/lucide/ellipsis';
  import FolderOpen from '~icons/lucide/folder-open';
  import GripVertical from '~icons/lucide/grip-vertical';
  import Pause from '~icons/lucide/pause';
  import Play from '~icons/lucide/play';
  import RotateCw from '~icons/lucide/rotate-cw';
  import type { DownloadId } from '@wirecatch/protocol';
  import { formatSpeed } from '@wirecatch/units';
  import {
    Button,
    cn,
    Icon,
    IconButton,
    Menu,
    type MenuTriggerProps,
    ProgressBar,
    textIfCut,
    tooltip,
  } from '@wirecatch/ui';
  import {
    FIX_LABEL,
    formatEta,
    formatPercent,
    formatSize,
    lineNote,
    shownWait,
    waitShort,
  } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import { canPause, canResume, canRetry } from '$lib/state/row-actions';
  import { groupOf } from '$lib/state/view';
  import CategoryIcon from '../CategoryIcon.svelte';
  import { rowProgress } from '../status';
  import { NUMBER, type Resizable } from './columns';
  import { rowMenu } from './row-menu';
  import { finishedNote, rowLine } from './row-text';

  interface Props {
    id: DownloadId;
    /** The row's place in the grid, the column headings first and group headings counted. */
    rowIndex: number;
    /** The number columns the list has room for; the rest show under the name. */
    shown: ReadonlySet<Resizable>;
    /** The list is in queue order, so rows drag up and down their queue. */
    canReorder: boolean;
  }

  let { id, rowIndex, shown, canReorder }: Props = $props();

  const app = getApp();
  const row = $derived(app.downloads.get(id));
  const selected = $derived(app.selection.has(id));
  const active = $derived(app.selection.active === id);
  const group = $derived(row ? groupOf(row.status) : null);
  const position = $derived(app.places.position.get(id));
  const queueName = $derived(row ? (app.queues.byId(row.queue)?.name ?? 'its queue') : '');
  const progress = $derived(row && row.status !== 'cancelled' ? rowProgress(row) : null);
  const percent = $derived(row ? formatPercent(row.done, row.total) : '');
  const size = $derived(row ? formatSize(row.done, row.total, row.status) : '');
  const speed = $derived(row && group === 'active' ? formatSpeed(row.speed) : '');
  const eta = $derived(row && group === 'active' ? formatEta(row.eta_secs) : '');
  /** Why a waiting download waits, or where it stands in its queue's line. */
  const waitNote = $derived.by(() => {
    if (!row || group !== 'waiting') return '';
    const wait = shownWait(row);
    if (wait) return waitShort(wait);
    return lineNote(app.places.ahead.get(id) ?? 0, queueName);
  });
  /** The line under the name: what went wrong, or where the file comes from and how it is going. */
  const meta = $derived(
    row && group
      ? rowLine(row, group, {
          waitNote,
          progressShown: shown.has('progress'),
          now: app.clock.now,
        })
      : '',
  );
  /** The figures of the number columns the list has no room for. */
  const folded = $derived(
    [
      !shown.has('progress') && percent && group !== 'finished' ? percent : '',
      !shown.has('size') ? size : '',
      !shown.has('speed') ? speed : '',
      !shown.has('left') && eta ? `${eta} left` : '',
    ].filter(Boolean),
  );
  const fix = $derived(row?.status === 'failed' ? row.error?.fix : undefined);
  const menu = $derived(rowMenu(app));

  /** The row's own buttons act on it alone. */
  function only() {
    if (!app.selection.has(id) || app.selection.size > 1) app.selection.only(id);
  }
</script>

{#if row}
  <div
    id="row-{id}"
    role="row"
    aria-selected={selected}
    aria-rowindex={rowIndex}
    data-id={id}
    draggable="true"
    class={cn(
      'group relative grid h-row grid-cols-list items-center gap-3 rounded-md px-3 text-ui transition-colors duration-fast ease-out',
      selected ? 'bg-accent-soft' : 'hover:bg-wash',
      active &&
        'group-focus-visible/list:before:absolute group-focus-visible/list:before:inset-y-3 group-focus-visible/list:before:left-0 group-focus-visible/list:before:w-0.75 group-focus-visible/list:before:rounded-full group-focus-visible/list:before:bg-accent forced-colors:group-focus-visible/list:before:bg-sys-highlight',
      active && !selected && 'group-focus-visible/list:bg-wash',
    )}
  >
    <span
      role="gridcell"
      class={cn(
        'relative flex h-full items-center justify-end pr-1 text-small tabular-nums',
        selected ? 'font-semibold text-accent' : 'text-ink-muted',
      )}
    >
      <span
        class={cn(
          'transition-opacity duration-fast',
          canReorder && position !== undefined && 'group-hover:opacity-0',
        )}>{position ?? ''}</span
      >
      {#if canReorder && position !== undefined}
        <span
          class="absolute inset-y-2 -right-1 -left-1 flex cursor-grab items-center justify-center rounded-sm text-ink-muted opacity-0 transition-opacity duration-fast group-hover:opacity-100 hover:bg-wash hover:text-ink"
          aria-hidden="true"
        >
          <Icon icon={GripVertical} size="sm" />
        </span>
      {/if}
    </span>
    <span role="gridcell" class="relative flex items-center">
      <CategoryIcon category={row.category} />
      <!-- For the pointer: the keyboard selects with Space on the list's own keys and Ctrl and Shift with the arrows. -->
      <button
        type="button"
        role="checkbox"
        tabindex={-1}
        aria-checked={selected}
        aria-label="Select {row.file_name}"
        class={cn(
          'absolute flex items-center justify-center border transition duration-fast ease-out',
          selected
            ? 'inset-0 rounded-md border-accent bg-accent text-accent-fg'
            : '-top-1 -right-1 size-4.5 rounded-xs border-line-strong bg-overlay text-transparent opacity-0 shadow-control group-hover:opacity-100 hover:border-accent',
        )}
        onclick={() => app.selection.toggle(id)}
      >
        <Icon icon={Check} size={selected ? 'sm' : 'xs'} />
      </button>
    </span>
    <span role="gridcell" class="flex min-w-0 flex-col justify-center gap-0.5">
      <span
        class="truncate font-medium text-ink"
        data-menu-anchor
        {@attach tooltip(() => textIfCut(row.file_name))}>{row.file_name}</span
      >
      <span class="flex min-w-0 gap-2 text-small">
        <span
          class={cn('truncate', group === 'failed' ? 'text-danger' : 'text-ink-muted')}
          {@attach tooltip(() => textIfCut(meta))}>{meta}</span
        >
        {#if folded.length > 0}
          <span class="shrink-0 text-ink-muted tabular-nums">{folded.join(' · ')}</span>
        {/if}
      </span>
      {#if !shown.has('progress') && progress && group !== 'finished' && group !== 'waiting'}
        <ProgressBar
          value={progress.value}
          state={progress.state}
          size="xs"
          label={row.file_name}
          valueText={progress.valueText}
          class="mt-0.5"
        />
      {/if}
    </span>
    {#if shown.has('progress')}
      <span role="gridcell" class="flex min-w-0 items-center gap-2 text-small tabular-nums">
        {#if group === 'waiting'}
          <span class="truncate text-ink-muted" {@attach tooltip(() => textIfCut(waitNote))}
            >{waitNote}</span
          >
        {:else if group === 'finished'}
          <span class="truncate text-ink-muted">{finishedNote(row, app.clock.now)}</span>
        {:else if progress}
          <ProgressBar
            value={progress.value}
            state={progress.state}
            size="sm"
            label={row.file_name}
            valueText={progress.valueText}
            class="min-w-0 flex-1"
          />
          <span class="w-9 shrink-0 text-right text-ink-muted">{percent}</span>
        {/if}
      </span>
    {/if}
    {#if shown.has('size')}<span role="gridcell" class={NUMBER}>{size}</span>{/if}
    {#if shown.has('speed')}
      <span
        role="gridcell"
        class={NUMBER}
        {@attach tooltip(() =>
          row.connections > 0 ? `${String(row.connections)} connections` : null,
        )}>{speed}</span
      >
    {/if}
    {#if shown.has('left')}<span role="gridcell" class={NUMBER}>{eta}</span>{/if}
    <!-- For the pointer: the keyboard reaches the same through the list's keys and its context menu, so these take no place in the tab order. -->
    <span role="gridcell" class="flex items-center justify-end gap-0.5">
      {#if fix}
        <Button
          size="sm"
          variant="ghost"
          class="absolute right-11 h-control-sm px-2.5 font-semibold text-accent"
          tabindex={-1}
          onclick={() => app.actions.fix(row.id, fix)}>{FIX_LABEL[fix]}</Button
        >
      {/if}
      <span
        class={cn(
          'flex items-center gap-0.5 transition-opacity duration-fast ease-out',
          group === 'failed' || selected ? 'opacity-100' : 'opacity-0 group-hover:opacity-100',
          active && 'group-focus-visible/list:opacity-100',
        )}
      >
        {#if !fix}
          {#if canPause(row)}
            <IconButton
              icon={Pause}
              label="Pause"
              size="sm"
              tabindex={-1}
              onclick={() => app.actions.pause([row.id])}
            />
          {:else if canResume(row)}
            <IconButton
              icon={Play}
              label="Resume"
              size="sm"
              tabindex={-1}
              onclick={() => app.actions.resume([row.id])}
            />
          {:else if row.status === 'completed'}
            <IconButton
              icon={FolderOpen}
              label="Show in folder"
              size="sm"
              tabindex={-1}
              onclick={() => app.actions.openFolder(row.id)}
            />
          {:else if canRetry(row)}
            <IconButton
              icon={RotateCw}
              label={row.status === 'cancelled' ? 'Start over' : 'Try again'}
              size="sm"
              tabindex={-1}
              onclick={() => app.actions.retry([row.id])}
            />
          {/if}
        {/if}
        <Menu entries={menu} label="Actions for {row.file_name}" align="end">
          {#snippet trigger(props: MenuTriggerProps)}
            <IconButton
              {...props}
              icon={Ellipsis}
              label="More actions"
              size="sm"
              tabindex={-1}
              onclick={() => {
                only();
                props.onclick();
              }}
            />
          {/snippet}
        </Menu>
      </span>
    </span>
  </div>
{/if}
