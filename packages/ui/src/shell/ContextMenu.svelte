<script lang="ts" module>
  import type { Placement } from '../lib/flyout';

  const PLACEMENT: Placement = { side: 'below', align: 'start', gap: 2, inset: 8 };
</script>

<script lang="ts">
  // A right-click menu for an area (the download list). It opens where the pointer is, or, from the keyboard (the Menu key or Shift+F10), at the focused element or the item it points at. Whatever it offers must also be reachable without it (D3).
  import { tick, type Snippet } from 'svelte';
  import { Anchored, dismissOnOutside } from '../lib/anchored.svelte';
  import { pointBox } from '../lib/flyout';
  import { focusedElement } from '../lib/focus';
  import type { MenuCloseReason, MenuEntry } from './menu';
  import MenuList from './MenuList.svelte';

  interface Props {
    entries: readonly MenuEntry[];
    label: string;
    /** Called before it opens, with what was right-clicked, so the owner can select it and set the entries. Returning false keeps the browser's own menu closed and this one too. */
    onopen?: (target: Element) => boolean | undefined;
    class?: string | undefined;
    children: Snippet;
  }

  let { entries, label, onopen, class: className, children }: Props = $props();

  let open = $state(false);
  let list = $state<HTMLElement | null>(null);
  let returnFocus: HTMLElement | null = null;
  const anchored = new Anchored();

  async function show(target: Element, x: number, y: number) {
    if (onopen?.(target) === false) return;
    returnFocus = focusedElement();
    open = true;
    await tick();
    if (list) anchored.place(pointBox(x, y), list, PLACEMENT);
  }

  function close(reason: MenuCloseReason) {
    open = false;
    // On Tab too: the browser's Tab then moves on from where the menu was opened.
    if (reason !== 'away' && returnFocus?.isConnected) returnFocus.focus();
  }

  function oncontextmenu(event: MouseEvent) {
    // The browser's own menu would offer Back and Reload, which mean nothing in an app.
    event.preventDefault();
    if (event.target instanceof Element) void show(event.target, event.clientX, event.clientY);
  }

  /** What has the keyboard: the focused element, or the item it points at (a grid's row, with `aria-activedescendant`), so the menu opens beside that item and is about it. */
  function keyboardTarget(focused: Element): Element {
    const id = focused.getAttribute('aria-activedescendant');
    return (id ? document.getElementById(id) : null) ?? focused;
  }

  function onkeydown(event: KeyboardEvent) {
    const menuKey = event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey);
    if (!menuKey || !(event.target instanceof Element)) return;
    event.preventDefault();
    const target = keyboardTarget(event.target);
    const box = target.getBoundingClientRect();
    void show(target, box.left, box.bottom);
  }

  $effect(() => {
    if (!open) return;
    return dismissOnOutside(
      () => [list],
      null,
      () => {
        close('away');
      },
    );
  });
</script>

<!-- The area only listens: every control in it keeps its own role, and the menu opens from the keyboard as well. -->
<div class={className} role="presentation" {oncontextmenu} {onkeydown}>
  {@render children()}
</div>

{#if open}
  <MenuList
    bind:element={list}
    {entries}
    {label}
    style={anchored.style}
    origin={anchored.above ? 'bottom' : 'top'}
    onclose={close}
  />
{/if}
