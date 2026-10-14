<script lang="ts">
  // The main window: the frameless title bar, the navigation beside the content, and the layers over them (dialogs, the command palette, toasts). The content sits on a translucent layer, so the window's material shows through around it. Settings, the dialogs and the palette are today's screens, shown as they are until they are rebuilt.
  import Download from '~icons/lucide/download';
  import { Icon, provideToaster, SidebarState, ToastRegion } from '@wirecatch/ui';
  import CommandPalette from '$lib/components/CommandPalette.svelte';
  import Dialogs from '$lib/components/Dialogs.svelte';
  import Settings from '$lib/components/Settings.svelte';
  import { getApp } from '$lib/state/app.svelte';
  import { followLook } from '$lib/state/appearance.svelte';
  import { keepSidebarCollapsed, sidebarCollapsed } from '$lib/state/sidebar-preference';
  import DownloadsView from './DownloadsView.svelte';
  import MainSidebar from './sidebar/MainSidebar.svelte';
  import MainTitleBar from './titlebar/MainTitleBar.svelte';
  import { windowInput } from './window-input';

  const app = getApp();
  provideToaster(app.toasts.toaster);
  followLook(app.appearance, () => app.settings);

  const sidebar = new SidebarState({ collapsed: sidebarCollapsed() });
  /** A link is being dragged over the window. */
  let linkOver = $state(false);
  const input = windowInput({
    app,
    layerOpen: () => sidebar.drawerOpen,
    linkOver: (over) => (linkOver = over),
  });

  function toggleSidebar() {
    sidebar.toggle();
    if (sidebar.wide) keepSidebarCollapsed(sidebar.collapsed);
  }

  // A dialog or the palette takes the window: the drawer, which keeps the rest of the page out of reach, gives way to it.
  $effect(() => {
    if (app.dialog !== null || app.paletteOpen) sidebar.closeDrawer();
  });
</script>

<svelte:head><title>{app.name}</title></svelte:head>

<!-- First in the page: the window's keys run before the details panel's own Escape, which then stands down for a key already used. -->
<svelte:window
  onkeydown={input.onkeydown}
  ondragover={input.ondragover}
  ondragleave={input.ondragleave}
  ondrop={input.ondrop}
/>
<svelte:document onpaste={input.onpaste} />

<div class="flex h-full flex-col">
  <MainTitleBar {sidebar} ontoggle={toggleSidebar} />
  <div class="flex min-h-0 flex-1">
    <MainSidebar {sidebar} />
    <main
      class="relative flex min-w-0 flex-1 flex-col overflow-hidden rounded-tl-lg border-t border-l border-line bg-layer"
      aria-label={app.screen === 'settings' ? 'Settings' : 'Downloads'}
    >
      {#if app.screen === 'settings'}
        <Settings />
      {:else}
        <DownloadsView />
      {/if}
      {#if linkOver}
        <!-- For the pointer, which is dragging: the drop lands on the window wherever it is. -->
        <div
          class="pointer-events-none absolute inset-2 z-scrim flex animate-fade-in items-center justify-center rounded-lg border-2 border-dashed border-accent bg-accent-soft"
          aria-hidden="true"
        >
          <div
            class="flex flex-col items-center gap-2 rounded-lg bg-base px-6 py-4 text-accent shadow-raised"
          >
            <Icon icon={Download} size="3xl" />
            <p class="text-subtitle font-semibold">Drop to download</p>
          </div>
        </div>
      {/if}
    </main>
  </div>
</div>

<Dialogs />
{#if app.sample}
  {@const sample = app.sample}
  <!-- The sample app's download windows float in the page; the real app opens real windows. -->
  {#await import('$lib/window/MockWindows.svelte') then { default: MockWindows }}
    <MockWindows registry={sample.windows} openWindow={app.openSampleWindow} />
  {/await}
{/if}
<CommandPalette />
<ToastRegion toaster={app.toasts.toaster} />
