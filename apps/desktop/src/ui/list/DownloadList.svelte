<script lang="ts">
  // The download list: a grid of every download the view shows, drawing only the rows on screen. The keyboard moves through it and selects as a file list does (one, a range with Shift, one more with Ctrl), the focus stays on the grid and points at its row, and the context menu offers what the selection can do. In a queue sorted by its order, rows drag up and down to reorder it; dragged onto a queue in the sidebar, they move there.
  import { isDownloadId, type DownloadId } from '@wirecatch/protocol';
  import { untrack } from 'svelte';
  import { cn, ContextMenu, FOCUS_RING_INSET } from '@wirecatch/ui';
  import { getDragIds, isDownloadDrag, setDragIds } from '$lib/dnd';
  import { dataIdAt } from '$lib/dom';
  import { listCommand } from '$lib/keymap';
  import { getApp } from '$lib/state/app.svelte';
  import { track } from '$lib/state/reactivity';
  import { CELL, COLUMNS } from './columns';
  import DownloadRow from './DownloadRow.svelte';
  import { listKeys } from './list-keys';
  import { rowMenu } from './row-menu';
  import {
    ASSUMED_ROW_HEIGHT,
    dropIndexAt,
    pageRows,
    rowRange,
    scrollToShow,
    type Viewport,
  } from './viewport';

  const app = getApp();

  /** The list's height before layout gives it one (and in tests): a typical window. */
  const ASSUMED_HEIGHT = 800;

  let scroller = $state<HTMLDivElement | null>(null);
  let rowsArea = $state<HTMLDivElement | null>(null);
  let scrollTop = $state(0);
  let dropIndex = $state<number | null>(null);
  // Sizes as layout gives them, fractions and borders included: `clientHeight` rounds and leaves the header's border out, which leaves the last row a pixel or two under the edge at some text sizes.
  let scrollerBox = $state<ResizeObserverSize[] | null>(null);
  let headerBox = $state<ResizeObserverSize[] | null>(null);
  /** A drawn row (`h-row`), measured, so placing rows agrees with drawing them at any text size. */
  let rowBox = $state<DOMRectReadOnly | null>(null);
  const height = $derived(scrollerBox?.[0]?.blockSize ?? 0);
  const header = $derived(headerBox?.[0]?.blockSize ?? 0);
  const measuredRow = $derived(rowBox?.height ?? 0);
  const rowHeight = $derived(measuredRow > 0 ? measuredRow : ASSUMED_ROW_HEIGHT);

  const ids = $derived(app.visible);
  const viewport = $derived<Viewport>({
    scrollTop,
    height: height > 0 ? height : ASSUMED_HEIGHT,
    header,
    row: rowHeight,
  });
  const range = $derived(rowRange(viewport, ids.length));
  const shown = $derived(ids.slice(range.start, range.end));
  // Only a row that is drawn can be pointed at (DUI-9): the keyboard row may have scrolled out of the drawn ones.
  const activeId = $derived.by(() => {
    const active = app.selection.active;
    return active !== null && shown.includes(active) ? `row-${String(active)}` : undefined;
  });
  const canReorder = $derived(app.view.kind === 'queue' && app.sort === 'queue');
  const menu = $derived(rowMenu(app));

  const keys = listKeys({ app, pageRows: () => pageRows(viewport), count: () => ids.length });

  // Keep the keyboard row on screen, but only when the keyboard row itself changes: the list changing underneath (a download finishing) must never move the view.
  $effect(() => {
    const active = app.selection.active;
    if (active !== null) untrack(() => reveal(active));
  });

  // A new view starts at the top.
  $effect(() => {
    track(app.view, app.search);
    if (scroller) scroller.scrollTop = 0;
  });

  function reveal(id: DownloadId) {
    const index = ids.indexOf(id);
    if (!scroller || index < 0) return;
    const now = { ...viewport, scrollTop: scroller.scrollTop };
    const to = scrollToShow(index, now);
    if (to !== null) scroller.scrollTop = to;
  }

  function idFrom(e: Event): DownloadId | null {
    const id = dataIdAt(e.target);
    return id !== null && isDownloadId(id) ? id : null;
  }

  /** A click on a row's own button is that button's, not a choice of rows. */
  function onButton(e: Event): boolean {
    return e.target instanceof Element && e.target.closest('button') !== null;
  }

  function onclick(e: MouseEvent) {
    const id = idFrom(e);
    if (id === null || onButton(e)) return;
    if (e.shiftKey) app.selection.range(id, e.ctrlKey || e.metaKey);
    else if (e.ctrlKey || e.metaKey) app.selection.toggle(id);
    else app.selection.only(id);
  }

  function ondblclick(e: MouseEvent) {
    const id = idFrom(e);
    const row = id === null || onButton(e) ? undefined : app.downloads.get(id);
    if (!row) return;
    if (row.status === 'completed') void app.actions.openFile(row.id);
    else void app.actions.togglePause([row.id]);
  }

  /** Before the menu opens: it is about the row it opened on, selected with the rest or alone. */
  function onmenu(target: Element): boolean {
    const id = dataIdAt(target);
    if (id === null || !isDownloadId(id)) return false;
    if (app.selection.has(id)) app.selection.active = id;
    else app.selection.only(id);
    return true;
  }

  /** Taking the keyboard with no row of its own (from Tab, or the search's down arrow), the list points at its first row on screen, without selecting it, as Explorer does, so a screen reader hears a row and the next arrow moves from there. */
  function onfocus() {
    const active = app.selection.active;
    if (active !== null && ids.includes(active)) return;
    const first = ids[Math.min(ids.length - 1, Math.ceil(scrollTop / rowHeight))];
    if (first !== undefined) app.selection.active = first;
  }

  function onkeydown(e: KeyboardEvent) {
    const command = listCommand(e);
    const run = command === null ? null : keys[command];
    if (!run) return;
    e.preventDefault();
    run(e);
  }

  function ondragstart(e: DragEvent) {
    const id = idFrom(e);
    if (id === null) return;
    if (!app.selection.has(id)) app.selection.only(id);
    setDragIds(e, app.selection.list);
  }

  function ondragover(e: DragEvent) {
    if (!canReorder || !isDownloadDrag(e) || !rowsArea) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    const top = rowsArea.getBoundingClientRect().top;
    dropIndex = dropIndexAt(e.clientY - top, rowHeight, ids.length);
  }

  /** The row a drop at `index` goes before: the first at or after it that is not itself moving. */
  function beforeDrop(index: number, moving: ReadonlySet<DownloadId>): DownloadId | null {
    return ids.slice(index).find((id) => !moving.has(id)) ?? null;
  }

  function ondrop(e: DragEvent) {
    if (!isDownloadDrag(e)) return;
    // Handled here, so the window does not read it as a dropped link.
    e.preventDefault();
    e.stopPropagation();
    const at = dropIndex;
    dropIndex = null;
    if (!canReorder || app.view.kind !== 'queue' || at === null) return;
    const moving = new Set(getDragIds(e));
    void app.actions.placeInQueue(app.view.id, [...moving], beforeDrop(at, moving));
  }
</script>

<ContextMenu
  entries={menu}
  label="Actions for the selected downloads"
  onopen={onmenu}
  class="flex min-h-0 flex-1 flex-col"
>
  <div
    bind:this={scroller}
    bind:borderBoxSize={scrollerBox}
    id="download-list"
    role="grid"
    tabindex="0"
    aria-label="Downloads"
    aria-multiselectable="true"
    aria-rowcount={ids.length + 1}
    aria-activedescendant={activeId}
    class={cn(
      'group/list @container/grid relative min-h-0 flex-1 overflow-y-auto overflow-anchor-none pb-2',
      FOCUS_RING_INSET,
    )}
    onscroll={() => (scrollTop = scroller?.scrollTop ?? 0)}
    {onclick}
    {ondblclick}
    {onfocus}
    {onkeydown}
    {ondragstart}
    {ondragover}
    ondragleave={() => (dropIndex = null)}
    ondragend={() => (dropIndex = null)}
    {ondrop}
  >
    <div
      role="rowgroup"
      bind:borderBoxSize={headerBox}
      class="sticky top-0 z-sticky border-b border-line bg-layer px-2 glass"
    >
      <div role="row" aria-rowindex={1} class="flex h-control-sm items-center gap-3 px-3">
        {#each COLUMNS as column (column.id)}
          <span
            role="columnheader"
            class={cn(CELL[column.id], 'text-caption font-semibold text-ink-subtle')}
          >
            {#if column.hidden}<span class="sr-only">{column.label}</span>{:else}{column.label}{/if}
          </span>
        {/each}
      </div>
    </div>
    <div
      bind:this={rowsArea}
      role="rowgroup"
      class="relative"
      style:height="{ids.length * rowHeight}px"
    >
      <div
        class="pointer-events-none invisible absolute h-row"
        aria-hidden="true"
        bind:contentRect={rowBox}
      ></div>
      <div
        class="absolute inset-x-0 top-0 px-2"
        style:transform="translateY({range.start * rowHeight}px)"
      >
        {#each shown as id, i (id)}
          <DownloadRow {id} index={range.start + i} />
        {/each}
      </div>
      {#if dropIndex !== null}
        <div
          class="pointer-events-none absolute inset-x-4 h-0.5 -translate-y-1/2 rounded-full bg-accent"
          style:top="{dropIndex * rowHeight}px"
        ></div>
      {/if}
    </div>
  </div>
</ContextMenu>
