<script lang="ts">
  // Right-click menu for the selected downloads. It only mirrors actions that are also visible
  // in the detail panel and the command palette (D3: nothing lives only in a context menu).
  import { onMount } from 'svelte';
  import { linkForDisplay } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import { isUnderWay } from '$lib/state/view';

  let { x, y, onclose }: { x: number; y: number; onclose: (refocusList: boolean) => void } =
    $props();

  /** The least room kept between the menu and the window's edges, in pixels. */
  const EDGE = 4;

  const app = getApp();
  let el = $state<HTMLDivElement>();
  // The menu's own size, measured once it is in the page (zero until then).
  let size = $state({ width: 0, height: 0 });

  interface Item {
    label: string;
    run: () => unknown;
    separator?: boolean;
  }

  // Built once, for the selection at the moment the menu opened.
  const items: Item[] = (() => {
    const rows = app.selection.rows;
    const ids = rows.map((r) => r.id);
    const one = rows.length === 1 ? rows[0] : undefined;
    const out: Item[] = [];
    if (rows.some((r) => isUnderWay(r.status))) {
      out.push({ label: 'Pause', run: () => app.pause(ids) });
    }
    if (rows.some((r) => r.status === 'paused'))
      out.push({ label: 'Resume', run: () => app.resume(ids) });
    if (rows.some((r) => r.status === 'failed' || r.status === 'cancelled')) {
      out.push({ label: 'Try again', run: () => app.retry(ids) });
    }
    if (one?.status === 'completed') out.push({ label: 'Open', run: () => app.openFile(one.id) });
    if (one) {
      out.push(
        { label: 'Show in folder', run: () => app.openFolder(one.id) },
        { label: 'Show progress window', run: () => app.showProgressWindow(one.id) },
        {
          label: 'Copy link',
          run: async () => {
            try {
              await navigator.clipboard.writeText(linkForDisplay(one.url));
              app.toasts.notify('Link copied.');
            } catch {
              app.toasts.fail(
                'Could not copy the link. Select it in the detail panel and copy it by hand.',
              );
            }
          },
        },
      );
    }
    const queues = app.queues.list.filter((q) => rows.some((r) => r.queue !== q.id));
    queues.forEach((q, i) =>
      out.push({
        label: `Move to ${q.name}`,
        run: () => app.moveToQueue(ids, q.id),
        separator: i === 0,
      }),
    );
    out.push({
      label: rows.length === 1 ? 'Remove' : `Remove ${rows.length} downloads`,
      run: () => app.openDialog({ kind: 'remove', ids }),
      separator: true,
    });
    return out;
  })();

  // Keep the menu inside the window, from its measured size rather than an assumed one.
  const left = $derived(Math.max(EDGE, Math.min(x, window.innerWidth - size.width - EDGE)));
  const top = $derived(Math.max(EDGE, Math.min(y, window.innerHeight - size.height - EDGE)));

  function buttons(): HTMLButtonElement[] {
    return el ? [...el.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')] : [];
  }

  onMount(() => {
    if (el) size = { width: el.offsetWidth, height: el.offsetHeight };
    buttons()[0]?.focus();
    const away = (e: PointerEvent) => {
      if (el && !el.contains(e.target as Node)) onclose(false);
    };
    window.addEventListener('pointerdown', away, true);
    return () => window.removeEventListener('pointerdown', away, true);
  });

  function onkeydown(e: KeyboardEvent) {
    const all = buttons();
    const i = all.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === 'ArrowDown') all[(i + 1) % all.length]?.focus();
    else if (e.key === 'ArrowUp') all[(i - 1 + all.length) % all.length]?.focus();
    else if (e.key === 'Home') all[0]?.focus();
    else if (e.key === 'End') all.at(-1)?.focus();
    else if (e.key === 'Escape' || e.key === 'Tab') onclose(true);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  function run(item: Item) {
    onclose(true);
    void item.run();
  }
</script>

<div
  bind:this={el}
  class="menu"
  role="menu"
  aria-label="Actions for the selected downloads"
  tabindex="-1"
  style:left="{left}px"
  style:top="{top}px"
  {onkeydown}
>
  {#each items as item (item.label)}
    {#if item.separator}<div class="sep" role="separator"></div>{/if}
    <button type="button" role="menuitem" tabindex="-1" onclick={() => run(item)}
      >{item.label}</button
    >
  {/each}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 30;
    width: max-content;
    min-width: 200px;
    padding: 4px;
    border: 1px solid var(--line);
    border-radius: var(--radius-m);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  button {
    display: block;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-s);
    background: transparent;
    color: var(--ink);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }

  button:hover,
  button:focus-visible {
    background: var(--accent-soft);
    outline: none;
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .sep {
    height: 1px;
    margin: 4px 2px;
    background: var(--line);
  }
</style>
