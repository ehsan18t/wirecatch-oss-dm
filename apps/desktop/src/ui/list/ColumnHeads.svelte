<script lang="ts">
  // The list's column headings. A click on one sorts the list by it inside each group: ascending, then descending, then back to queue order (#). The line at a number column's left edge drags to resize it, and a double click puts it back.
  import ChevronDown from '~icons/lucide/chevron-down';
  import { cn, Icon } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';
  import { nextSort, SORT_LABEL, type SortColumn } from '$lib/state/view';
  import {
    DEFAULT_WIDTH,
    HEADINGS,
    MIN_WIDTH,
    MOST_WIDTH,
    remInPixels,
    type Resizable,
  } from './columns';

  interface Props {
    shown: ReadonlySet<Resizable>;
    /** A column's new width in pixels, or undefined to put it back; `done` when the drag ends. */
    onresize: (column: Resizable, width: number | undefined, done: boolean) => void;
  }

  let { shown, onresize }: Props = $props();

  const app = getApp();

  const headings = $derived(HEADINGS.filter((h) => h.resize === undefined || shown.has(h.resize)));

  function ariaSort(column: SortColumn | undefined): 'ascending' | 'descending' | 'none' {
    if (column === undefined || app.sort.column !== column) return 'none';
    return app.sort.descending ? 'descending' : 'ascending';
  }

  let dragging = $state<Resizable | null>(null);

  function startResize(event: PointerEvent & { currentTarget: HTMLElement }, column: Resizable) {
    const handle = event.currentTarget;
    const cell = handle.parentElement;
    if (event.button !== 0 || !cell) return;
    event.preventDefault();
    handle.setPointerCapture(event.pointerId);
    const from = event.clientX;
    const start = cell.getBoundingClientRect().width;
    const rem = remInPixels();
    const least = MIN_WIDTH[column] * rem;
    const most = DEFAULT_WIDTH[column] * rem * MOST_WIDTH;
    dragging = column;
    let last = start;
    const move = (e: PointerEvent) => {
      // The handle is on the column's left edge: dragging left widens it.
      last = Math.round(Math.min(most, Math.max(least, start + from - e.clientX)));
      onresize(column, last, false);
    };
    const end = () => {
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', end);
      handle.removeEventListener('pointercancel', end);
      dragging = null;
      onresize(column, last, true);
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', end);
    handle.addEventListener('pointercancel', end);
  }
</script>

{#each headings as heading (heading.id)}
  {@const resize = heading.resize}
  {@const sort = heading.sort}
  <span
    role="columnheader"
    aria-sort={sort ? ariaSort(sort) : undefined}
    class={cn(
      'relative flex h-full min-w-0 items-center',
      heading.numeric && 'justify-end',
      heading.id === 'position' && 'pr-1',
    )}
  >
    {#if heading.hidden}
      <span class="sr-only">{heading.label}</span>
    {:else if sort}
      {@const on = app.sort.column === sort}
      <button
        type="button"
        tabindex={-1}
        title={heading.title
          ? `${heading.title}. Sort by ${heading.title.toLowerCase()}`
          : `Sort by ${SORT_LABEL[sort].toLowerCase()}`}
        class={cn(
          'group/head -mx-1.5 flex h-control-xs min-w-0 items-center gap-0.5 rounded-sm px-1.5 text-caption whitespace-nowrap transition-colors duration-fast ease-out hover:bg-wash hover:text-ink',
          on ? 'font-semibold text-ink' : 'text-ink-subtle',
        )}
        onclick={() => (app.sort = nextSort(app.sort, sort))}
      >
        <span class="truncate">{heading.label}</span>
        <Icon
          icon={ChevronDown}
          size="xs"
          class={cn(
            'shrink-0 transition duration-normal ease-out',
            on
              ? cn('text-accent opacity-100', !app.sort.descending && 'rotate-180')
              : 'opacity-0 group-hover/head:opacity-quiet',
          )}
        />
      </button>
    {/if}
    {#if resize}
      <span
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize {heading.label}"
        title="Drag to resize, double-click to reset"
        class={cn(
          'absolute inset-y-1.5 -left-2 w-2 cursor-col-resize touch-none after:absolute after:inset-y-0 after:left-0.75 after:w-px after:transition after:duration-fast',
          dragging === resize
            ? 'after:w-0.5 after:bg-accent'
            : 'after:bg-line after:opacity-0 group-hover/heads:after:opacity-100 hover:after:w-0.5 hover:after:bg-accent',
        )}
        onpointerdown={(e) => startResize(e, resize)}
        ondblclick={() => onresize(resize, undefined, true)}
      ></span>
    {/if}
  </span>
{/each}
