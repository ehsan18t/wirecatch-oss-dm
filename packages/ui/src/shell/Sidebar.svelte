<script lang="ts">
  // The window's navigation. It is one Tab stop, the place shown, with the arrows moving through every place (nav-keys.ts). Docked, it is part of the window; as a drawer it is a modal dialog: named, marked aria-modal, with the rest of the page inert (as a native modal dialog makes it; the drawer cannot be one, since the same items render docked), it takes focus, keeps Tab inside, closes on Escape or a click beside it, and hands focus back.
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import { findElement } from '../lib/dom';
  import {
    containTab,
    focusedElement,
    inertOutside,
    initialFocus,
    restoreFocus,
  } from '../lib/focus';
  import ModalAnnouncer from './ModalAnnouncer.svelte';
  import { glide } from '../motion/motion';
  import { CHROME } from '../recipes/control';
  import { PANEL_SURFACE } from '../recipes/surface';
  import { keepOneStop, navKey } from './nav-keys';
  import { setSidebarContext, type SidebarState } from './sidebar-state.svelte';

  interface Props {
    sidebar: SidebarState;
    /** The navigation's name for screen readers. */
    label: string;
    /** Above the items: an "Add download" button, say. */
    header?: Snippet;
    /** Pinned under the items: settings. */
    footer?: Snippet;
    class?: string | undefined;
    children: Snippet;
  }

  let { sidebar, label, header, footer, class: className, children }: Props = $props();

  let panel = $state<HTMLElement | null>(null);
  let scrim = $state<HTMLElement | null>(null);
  let current: HTMLElement | null = null;

  setSidebarContext({
    get showLabels() {
      return sidebar.showLabels;
    },
    activated(item) {
      if (current?.isConnected && current !== item) glide(current, item, { axis: 'y' });
      current = item;
    },
    chose() {
      sidebar.closeDrawer();
    },
  });

  $effect(() => {
    if (!sidebar.drawerOpen || !panel) return;
    const opener = focusedElement();
    const drawer = panel;
    const release = inertOutside(scrim ? [drawer, scrim] : [drawer]);
    (findElement(drawer, '[aria-current="page"]') ?? initialFocus(drawer)).focus();
    return () => {
      release();
      restoreFocus(opener, drawer);
    };
  });

  // One Tab stop, kept as places come and go and as the place shown changes.
  $effect(() => {
    if (!panel) return;
    const root = panel;
    keepOneStop(root);
    const observer = new MutationObserver(() => {
      keepOneStop(root);
    });
    observer.observe(root, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ['aria-current'],
    });
    return () => {
      observer.disconnect();
    };
  });

  function onNavKey(event: KeyboardEvent) {
    if (!event.defaultPrevented && panel && navKey(event, panel)) event.preventDefault();
  }

  function onkeydown(event: KeyboardEvent) {
    if (!sidebar.drawerOpen || !panel || event.defaultPrevented) return;
    // Keys meant for something above the drawer, such as a dialog, are not the drawer's.
    const target = event.target;
    if (!(target instanceof Node) || (target !== document.body && !panel.contains(target))) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      sidebar.closeDrawer();
    } else {
      containTab(event, panel);
    }
  }
</script>

<svelte:window {onkeydown} />

{#if sidebar.drawerOpen}
  <!-- For the pointer only: Escape closes the drawer from the keyboard. -->
  <button
    bind:this={scrim}
    type="button"
    tabindex="-1"
    aria-label="Close navigation"
    class="fixed inset-0 z-scrim animate-fade-in cursor-default bg-scrim"
    onclick={() => sidebar.closeDrawer()}
  ></button>
{/if}

<nav
  aria-label={label}
  class={cn(
    'relative h-full shrink-0 transition-all duration-slow ease-out',
    sidebar.mode === 'expanded' ? 'w-sidebar' : 'w-rail',
    CHROME,
    className,
  )}
>
  <div
    bind:this={panel}
    role={sidebar.drawerOpen ? 'dialog' : undefined}
    aria-modal={sidebar.drawerOpen || undefined}
    aria-label={sidebar.drawerOpen ? label : undefined}
    data-layer={sidebar.drawerOpen ? '' : undefined}
    onkeydown={onNavKey}
    class={cn(
      'flex h-full flex-col',
      sidebar.drawerOpen
        ? cn(
            PANEL_SURFACE,
            'absolute inset-y-0 left-0 z-drawer w-drawer animate-rise-in rounded-r-lg border-r',
          )
        : 'w-full',
    )}
  >
    {#if header}
      <div class="flex shrink-0 flex-col gap-1 px-2 pt-1 pb-2">{@render header()}</div>
    {/if}
    <div class="flex min-h-0 flex-1 flex-col gap-0.5 overflow-x-hidden overflow-y-auto px-2 pb-2">
      {@render children()}
    </div>
    {#if footer}
      <div class="flex shrink-0 flex-col gap-0.5 border-t border-line px-2 py-2">
        {@render footer()}
      </div>
    {/if}
    {#if sidebar.drawerOpen}<ModalAnnouncer />{/if}
  </div>
</nav>
