<script lang="ts">
  // The download list: a grid of every download the view shows, in groups by state under their headings, drawing only what is on screen. A click on a row opens its details; Ctrl and Shift select as a file list does, and so does the keyboard, whose focus stays on the grid and points at its row. In queue order, rows drag up and down their queue; dragged onto a queue in the sidebar, they move there.
  import { isDownloadId, type DownloadId } from '@wirecatch/protocol';
  import { untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { cn, ContextMenu, FOCUS_RING_INSET } from '@wirecatch/ui';
  import { getDragIds, isDownloadDrag, setDragIds } from '$lib/dnd';
  import { dataIdAt } from '$lib/dom';
  import { listCommand } from '$lib/keymap';
  import { getApp } from '$lib/state/app.svelte';
  import { dropTarget } from '$lib/state/order';
  import { track } from '$lib/state/reactivity';
  import { groupOf, isInLine, type ListSection } from '$lib/state/view';
  import { keepWidths, storedWidths } from './column-widths';
  import ColumnHeads from './ColumnHeads.svelte';
  import { gridColumns, shownColumns, type Resizable, type Widths } from './columns';
  import DownloadRow from './DownloadRow.svelte';
  import GroupHeading from './GroupHeading.svelte';
  import { listKeys } from './list-keys';
  import { rowMenu } from './row-menu';
  import {
    ASSUMED_GROUP_HEIGHT,
    ASSUMED_ROW_HEIGHT,
    dropIndexAt,
    dropLineAt,
    geometry,
    itemRange,
    pageRows,
    scrollToShow,
    type Viewport,
  } from './viewport';

  const app = getApp();

  /** The list's height before layout gives it one (and in tests): a typical window. */
  const ASSUMED_HEIGHT = 800;

  let scroller = $state<HTMLDivElement | null>(null);
  let rowsArea = $state<HTMLDivElement | null>(null);
  let scrollTop = $state(0);
  /** Where a drag would drop, and the group it moves in. */
  let drop = $state<{ index: number; section: ListSection } | null>(null);
  // Sizes as layout gives them, fractions and borders included: `clientHeight` rounds and leaves the header's border out, which leaves the last row a pixel or two under the edge at some text sizes.
  let scrollerBox = $state<ResizeObserverSize[] | null>(null);
  let headerBox = $state<ResizeObserverSize[] | null>(null);
  /** A drawn row (`h-row`) and heading (`h-group`), measured, so placing them agrees with drawing them at any text size. */
  let rowBox = $state<DOMRectReadOnly | null>(null);
  let groupBox = $state<DOMRectReadOnly | null>(null);
  const height = $derived(scrollerBox?.[0]?.blockSize ?? 0);
  const width = $derived(scrollerBox?.[0]?.inlineSize ?? 0);
  const header = $derived(headerBox?.[0]?.blockSize ?? 0);
  const rowHeight = $derived(rowBox && rowBox.height > 0 ? rowBox.height : ASSUMED_ROW_HEIGHT);
  const groupHeight = $derived(
    groupBox && groupBox.height > 0 ? groupBox.height : ASSUMED_GROUP_HEIGHT,
  );

  const layout = $derived(app.layout);
  const ids = $derived(layout.ids);
  const geo = $derived(geometry(layout, rowHeight, groupHeight));
  const viewport = $derived<Viewport>({
    scrollTop,
    height: height > 0 ? height : ASSUMED_HEIGHT,
    header,
    row: rowHeight,
  });
  const range = $derived(itemRange(geo, viewport));
  const items = $derived(geo.items.slice(range.start, range.end));
  const shown = $derived(items.flatMap((item) => (item.kind === 'row' ? [item.id] : [])));

  let widths = $state<Widths>(storedWidths());
  const columns = $derived(shownColumns(width, widths));
  const template = $derived(gridColumns(columns, widths));

  function onresize(column: Resizable, to: number | undefined, done: boolean) {
    const next: Widths = Object.fromEntries(
      Object.entries(widths).filter(([key]) => key !== column),
    );
    if (to !== undefined) next[column] = to;
    widths = next;
    if (done) keepWidths(next);
  }

  // A drawn row that joins the list while it shows the same view and search pops in (D46); a row that only scrolls into sight, or a new view's or search's rows, do not. Each row's flag clears when its animation ends.
  const fresh = new SvelteSet<DownloadId>();
  let before: { scope: string; ids: SvelteSet<DownloadId> } | null = null;
  $effect(() => {
    const now = ids;
    const scope = JSON.stringify([app.view, app.search]);
    untrack(() => {
      if (before?.scope === scope) {
        const was = before.ids;
        const drawn = new SvelteSet(shown);
        for (const id of now) if (!was.has(id) && drawn.has(id)) fresh.add(id);
      } else {
        fresh.clear();
      }
      before = { scope, ids: new SvelteSet(now) };
    });
  });
  // Only a row that is drawn can be pointed at (DUI-9): the keyboard row may have scrolled out of the drawn ones.
  const activeId = $derived.by(() => {
    const active = app.selection.active;
    return active !== null && shown.includes(active) ? `row-${String(active)}` : undefined;
  });
  const canReorder = $derived(app.sort.column === 'position');
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
    const to = scrollToShow(geo, index, { ...viewport, scrollTop: scroller.scrollTop });
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
    else app.openDetails(id);
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
    const first = geo.items.find(
      (item, i) => item.kind === 'row' && (geo.tops[i] ?? 0) >= scrollTop,
    );
    if (first?.kind === 'row') app.selection.active = first.id;
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

  /** Where rows of one queue dropped at `index` go in its line, or undefined where a drop there says nothing about the order: rows move only within their own group, among rows of their own queue. */
  function placeFor(moving: ReadonlySet<DownloadId>, index: number) {
    const rows = [...moving].flatMap((id) => app.downloads.get(id) ?? []);
    const first = rows[0];
    if (!first || rows.some((r) => r.queue !== first.queue || !isInLine(r.status))) return;
    const group = groupOf(first.status);
    const section = layout.sections.find((s) => s.group === null || s.group === group);
    if (!section || index < section.start || index > section.start + section.size) return;
    const onScreen = ids.slice(section.start, section.start + section.size);
    const line = app.downloads
      .idsInQueue(first.queue)
      .filter((id) => isInLine(app.downloads.get(id)?.status ?? 'completed'));
    const target = dropTarget(onScreen, index - section.start, line, moving);
    return target === undefined ? undefined : { queue: first.queue, before: target, section };
  }

  function ondragover(e: DragEvent) {
    if (!canReorder || !isDownloadDrag(e) || !rowsArea) return;
    // A drag's ids cannot be read until the drop; the drag started from the selection.
    const index = dropIndexAt(geo, e.clientY - rowsArea.getBoundingClientRect().top);
    const place = placeFor(new Set(app.selection.list), index);
    if (place === undefined) {
      drop = null;
      return;
    }
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    drop = { index, section: place.section };
  }

  function ondrop(e: DragEvent) {
    if (!isDownloadDrag(e)) return;
    // Handled here, so the window does not read it as a dropped link.
    e.preventDefault();
    e.stopPropagation();
    const at = drop;
    drop = null;
    if (!canReorder || at === null) return;
    const moving = new Set(getDragIds(e));
    const place = placeFor(moving, at.index);
    if (place) void app.actions.placeInQueue(place.queue, [...moving], place.before);
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
    aria-rowcount={geo.items.length + 1}
    aria-activedescendant={activeId}
    style:--list-columns={template}
    class={cn(
      'group/list relative min-h-0 flex-1 overflow-y-auto overflow-anchor-none pb-16',
      FOCUS_RING_INSET,
    )}
    onscroll={() => (scrollTop = scroller?.scrollTop ?? 0)}
    {onclick}
    {onfocus}
    {onkeydown}
    {ondragstart}
    {ondragover}
    ondragleave={() => (drop = null)}
    ondragend={() => (drop = null)}
    {ondrop}
  >
    <!-- The base under the content layer's tone, so rows never show through the header. -->
    <div role="rowgroup" bind:borderBoxSize={headerBox} class="sticky top-0 z-sticky bg-base">
      <div class="border-b border-line bg-layer px-2">
        <div
          role="row"
          aria-rowindex={1}
          class="group/heads grid h-control-sm grid-cols-list items-center gap-3 px-3"
        >
          <ColumnHeads shown={columns} {onresize} />
        </div>
      </div>
    </div>
    <div
      bind:this={rowsArea}
      role="rowgroup"
      class="relative"
      style:height="{geo.tops.at(-1) ?? 0}px"
    >
      <div
        class="pointer-events-none invisible absolute h-row"
        aria-hidden="true"
        bind:contentRect={rowBox}
      ></div>
      <div
        class="pointer-events-none invisible absolute h-group"
        aria-hidden="true"
        bind:contentRect={groupBox}
      ></div>
      <div
        class="absolute inset-x-0 top-0 px-2"
        style:transform="translateY({geo.tops[range.start] ?? 0}px)"
      >
        <!-- Headings count as rows, so a screen reader's place in the grid matches what shows. -->
        {#each items as item, i (item.kind === 'row' ? item.id : `group-${String(item.section.group)}`)}
          {#if item.kind === 'group' && item.section.group !== null}
            <GroupHeading
              section={item.section}
              group={item.section.group}
              rowIndex={range.start + i + 2}
            />
          {:else if item.kind === 'row'}
            <div
              class={fresh.has(item.id) ? 'animate-pop-in' : undefined}
              onanimationend={() => fresh.delete(item.id)}
            >
              <DownloadRow
                id={item.id}
                rowIndex={range.start + i + 2}
                shown={columns}
                {canReorder}
              />
            </div>
          {/if}
        {/each}
      </div>
      {#if drop !== null}
        <div
          class="pointer-events-none absolute inset-x-4 h-0.5 -translate-y-1/2 rounded-full bg-accent"
          style:top="{dropLineAt(geo, drop.index, drop.section)}px"
        ></div>
      {/if}
    </div>
  </div>
</ContextMenu>
