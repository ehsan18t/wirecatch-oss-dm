<script lang="ts">
  import type { DownloadId } from '@wirecatch/protocol';
  import { untrack } from 'svelte';
  import { getDragIds, isDownloadDrag, setDragIds } from '$lib/dnd';
  import { ROW_HEIGHT } from '$lib/layout';
  import { getApp } from '$lib/state/app.svelte';
  import { HINT_TEXT, type HintId } from '$lib/state/hints.svelte';
  import { track } from '$lib/state/reactivity';
  import Button from './Button.svelte';
  import ContextMenu from './ContextMenu.svelte';
  import DownloadRow from './DownloadRow.svelte';
  import EmptyState from './EmptyState.svelte';

  const app = getApp();

  const OVERSCAN = 6;
  /** The list's height before layout gives it one (and in tests): a typical window. */
  const ASSUMED_HEIGHT = 800;

  let viewport = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(0);
  let dropIndex = $state<number | null>(null);
  let menu = $state<{ x: number; y: number } | null>(null);

  const ids = $derived(app.visible);
  const viewHeight = $derived(height > 0 ? height : ASSUMED_HEIGHT);
  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN));
  const end = $derived(
    Math.min(ids.length, Math.ceil((scrollTop + viewHeight) / ROW_HEIGHT) + OVERSCAN),
  );
  const windowIds = $derived(ids.slice(start, end));
  const canReorder = $derived(app.view.kind === 'queue' && app.sort === 'queue');

  // One hint at a time, only when it applies to what is on screen.
  const hint = $derived.by((): HintId | null => {
    if (ids.length === 0) return null;
    const candidates: HintId[] = [];
    if (canReorder && ids.length > 1) candidates.push('reorder');
    if (app.queues.length > 1) candidates.push('drag-to-queue');
    candidates.push('palette');
    return candidates.find((h) => app.hints.isOpen(h)) ?? null;
  });

  /** Scrolls just enough to show a row. */
  function reveal(id: DownloadId) {
    const el = viewport;
    if (!el) return;
    const i = ids.indexOf(id);
    if (i < 0) return;
    const top = i * ROW_HEIGHT;
    if (top < el.scrollTop) el.scrollTop = top;
    else if (top + ROW_HEIGHT > el.scrollTop + el.clientHeight)
      el.scrollTop = top + ROW_HEIGHT - el.clientHeight;
  }

  // Keep the keyboard row on screen, but only when the keyboard row itself changes. The list
  // changing underneath (a download finishing, say) must never yank the view back.
  $effect(() => {
    const active = app.active;
    if (active !== null) untrack(() => reveal(active));
  });

  // A new view starts at the top.
  $effect(() => {
    track(app.view, app.search);
    if (viewport) viewport.scrollTop = 0;
  });

  function idFrom(e: Event): DownloadId | null {
    const el = (e.target as HTMLElement | null)?.closest<HTMLElement>('[data-id]');
    return el ? Number(el.dataset.id) : null;
  }

  function onclick(e: MouseEvent) {
    const id = idFrom(e);
    if (id === null) return;
    if (e.shiftKey) app.selectRange(id, e.ctrlKey || e.metaKey);
    else if (e.ctrlKey || e.metaKey) app.toggle(id);
    else app.selectOnly(id);
  }

  function ondblclick(e: MouseEvent) {
    const id = idFrom(e);
    const row = id === null ? undefined : app.downloads.get(id);
    if (!row) return;
    if (row.status === 'completed') void app.openFile(row.id);
    else void app.togglePause([row.id]);
  }

  function oncontextmenu(e: MouseEvent) {
    const id = idFrom(e);
    if (id === null) return;
    e.preventDefault();
    if (!app.selection.has(id)) app.selectOnly(id);
    else app.active = id;
    menu = { x: e.clientX, y: e.clientY };
  }

  /** Opens the menu from the keyboard, next to the keyboard row. */
  function openMenuAtActive() {
    if (app.active === null || !viewport) return;
    if (!app.selection.has(app.active)) app.selectOnly(app.active);
    const row = document.getElementById(`row-${app.active}`);
    const rowBox = (row ?? viewport).getBoundingClientRect();
    // Under the row, lined up with its name.
    const nameBox = row?.querySelector('[data-menu-anchor]')?.getBoundingClientRect() ?? rowBox;
    menu = { x: nameBox.left, y: rowBox.bottom };
  }

  function onkeydown(e: KeyboardEvent) {
    const ctrl = e.ctrlKey || e.metaKey;
    const page = Math.max(1, Math.floor(viewHeight / ROW_HEIGHT) - 1);
    if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
      e.preventDefault();
      void app.nudge(e.key === 'ArrowUp' ? -1 : 1);
      return;
    }
    if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
      e.preventDefault();
      openMenuAtActive();
      return;
    }
    switch (e.key) {
      case 'ArrowDown':
        app.moveActive(1, e.shiftKey);
        break;
      case 'ArrowUp':
        app.moveActive(-1, e.shiftKey);
        break;
      case 'PageDown':
        app.moveActive(page, e.shiftKey);
        break;
      case 'PageUp':
        app.moveActive(-page, e.shiftKey);
        break;
      case 'Home':
        app.moveActive(-ids.length, e.shiftKey);
        break;
      case 'End':
        app.moveActive(ids.length, e.shiftKey);
        break;
      case ' ':
        if (app.selection.size === 0 && app.active !== null) app.selectOnly(app.active);
        void app.togglePause();
        break;
      case 'Enter': {
        const only = app.selectedRows.length === 1 ? app.selectedRows[0] : undefined;
        if (only?.status === 'completed') void app.openFile(only.id);
        // On a download that needs attention, Enter runs the fix its row offers.
        else if (only?.status === 'failed' && only.error?.fix)
          void app.fix(only.id, only.error.fix);
        break;
      }
      case 'Delete':
        if (app.selection.size > 0) app.openDialog({ kind: 'remove', ids: app.selectedIds });
        break;
      case 'a':
      case 'A':
        if (!ctrl) return;
        app.selectAll();
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  function ondragstart(e: DragEvent) {
    const id = idFrom(e);
    if (id === null) return;
    if (!app.selection.has(id)) app.selectOnly(id);
    setDragIds(e, app.selectedIds);
  }

  function ondragover(e: DragEvent) {
    if (!canReorder || !isDownloadDrag(e) || !viewport) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    const rect = viewport.getBoundingClientRect();
    const y = e.clientY - rect.top + viewport.scrollTop;
    dropIndex = Math.max(0, Math.min(ids.length, Math.round(y / ROW_HEIGHT)));
  }

  function ondrop(e: DragEvent) {
    if (!isDownloadDrag(e)) return;
    // Handled here, so the window does not treat it as a dropped link.
    e.preventDefault();
    e.stopPropagation();
    if (!canReorder || app.view.kind !== 'queue' || dropIndex === null) return;
    const moving = new Set(getDragIds(e));
    // Drop before the first row at or after the line that is not itself being moved.
    let before: DownloadId | null = null;
    for (let i = dropIndex; i < ids.length; i++) {
      const id = ids[i];
      if (id !== undefined && !moving.has(id)) {
        before = id;
        break;
      }
    }
    void app.placeInQueue(app.view.id, [...moving], before);
    dropIndex = null;
  }
</script>

<div class="wrap">
  {#if hint}
    <div class="hint" role="note">
      <p>{HINT_TEXT[hint]}</p>
      <Button small onclick={() => app.hints.dismiss(hint)}>Got it</Button>
    </div>
  {/if}

  {#if ids.length > 0}
    <div class="header" aria-hidden="true">
      <span></span>
      <span>Name</span>
      <span class="r">Size</span>
      <span class="r">Done</span>
      <span class="r speed">Speed</span>
      <span class="r eta">Time left</span>
      <span>Status</span>
    </div>
  {/if}

  <div
    bind:this={viewport}
    bind:clientHeight={height}
    id="download-list"
    class="list"
    class:hidden={ids.length === 0}
    class:menu-open={menu !== null}
    role="grid"
    tabindex="0"
    aria-label="Downloads"
    aria-multiselectable="true"
    aria-rowcount={ids.length}
    aria-activedescendant={app.active !== null && ids.includes(app.active)
      ? `row-${app.active}`
      : undefined}
    onscroll={() => (scrollTop = viewport?.scrollTop ?? 0)}
    {onclick}
    {ondblclick}
    {oncontextmenu}
    {onkeydown}
    {ondragstart}
    {ondragover}
    ondragleave={() => (dropIndex = null)}
    ondragend={() => (dropIndex = null)}
    {ondrop}
  >
    <div class="canvas" style:height="{ids.length * ROW_HEIGHT}px">
      <div class="window" style:transform="translateY({start * ROW_HEIGHT}px)">
        {#each windowIds as id, i (id)}
          <DownloadRow {id} index={start + i} dropBefore={dropIndex === start + i} />
        {/each}
      </div>
      {#if dropIndex !== null && dropIndex === ids.length}
        <div class="drop-end" style:top="{ids.length * ROW_HEIGHT - 1}px"></div>
      {/if}
    </div>
  </div>

  {#if ids.length === 0 && app.ready}
    <EmptyState />
  {/if}
</div>

{#if menu}
  <ContextMenu
    x={menu.x}
    y={menu.y}
    onclose={(refocus) => {
      menu = null;
      if (refocus) viewport?.focus();
    }}
  />
{/if}

<style>
  .wrap {
    position: relative;
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    /* Columns: icon, name, size, done, speed, time left, status. */
    --list-columns: 20px minmax(180px, 1fr) 128px 40px 82px 72px 108px;
  }

  /* Narrow windows drop time left first, then speed; the name always keeps real room. */
  @container main (max-width: 899px) {
    .wrap {
      --list-columns: 20px minmax(180px, 1fr) 128px 40px 82px 108px;
    }

    .header .eta {
      display: none;
    }
  }

  @container main (max-width: 759px) {
    .wrap {
      --list-columns: 20px minmax(180px, 1fr) 120px 40px 104px;
    }

    .header .speed {
      display: none;
    }
  }

  .hint {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 10px 16px 2px;
    padding: 8px 10px 8px 14px;
    border: 1px solid var(--line);
    border-left: 3px solid var(--accent);
    border-radius: var(--radius-m);
    background: var(--surface-2);
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .hint p {
    flex: 1;
    margin: 0;
  }

  .header {
    display: grid;
    grid-template-columns: var(--list-columns);
    column-gap: 12px;
    padding: 10px 16px 6px 18px;
    border-bottom: 1px solid var(--line);
    color: var(--ink-3);
    font-size: var(--text-xs);
    font-weight: 600;
    /* Same gutter as the list below, so columns line up whether or not it scrolls. */
    overflow-y: hidden;
    scrollbar-gutter: stable;
  }

  .r {
    text-align: right;
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    scrollbar-gutter: stable;
    contain: strict;
    outline: none;
  }

  .list.hidden {
    flex: 0;
  }

  .list:focus-visible {
    box-shadow: inset 0 0 0 1px var(--line-strong);
  }

  .canvas {
    position: relative;
  }

  .window {
    will-change: transform;
  }

  .drop-end {
    position: absolute;
    right: 0;
    left: 0;
    height: 2px;
    background: var(--accent);
  }
</style>
