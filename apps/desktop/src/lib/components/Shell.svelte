<script lang="ts">
  import { PRODUCT_NAME } from '@wirecatch/product';
  import { onMount } from 'svelte';
  import { runKey } from '$lib/commands';
  import { isDownloadDrag } from '$lib/dnd';
  import { isEditable, windowCommand } from '$lib/keymap';
  import { DOCK_WIDTH } from '$lib/layout';
  import { getApp } from '$lib/state/app.svelte';
  import { followTheme } from '$lib/theme.svelte';
  import Button from './Button.svelte';
  import CommandPalette from './CommandPalette.svelte';
  import DetailPanel from './DetailPanel.svelte';
  import Dialogs from './Dialogs.svelte';
  import DownloadList from './DownloadList.svelte';
  import Settings from './Settings.svelte';
  import Sidebar from './Sidebar.svelte';
  import StorageBanner from './StorageBanner.svelte';
  import Toasts from './Toasts.svelte';
  import Toolbar from './Toolbar.svelte';

  const app = getApp();

  // Below DOCK_WIDTH the detail panel floats over the list instead of taking a column.
  onMount(() => {
    const query = window.matchMedia(`(min-width: ${DOCK_WIDTH}px)`);
    const update = () => (app.wide = query.matches);
    update();
    query.addEventListener('change', update);
    return () => query.removeEventListener('change', update);
  });

  followTheme(() => app.settings?.theme);

  const detail = $derived.by((): 'docked' | 'overlay' | null => {
    if (app.screen !== 'downloads') return null;
    if (app.wide) return app.detailsOpen ? 'docked' : null;
    return app.selection.size > 0 ? 'overlay' : null;
  });

  // A visible dialog takes the keyboard. Questions have their own windows (D23).
  const blocked = $derived(app.dialog !== null);

  /** A key typed in one of the sample app's download windows, which float in this page. */
  function inDownloadWindow(e: Event): boolean {
    return e.target instanceof Element && e.target.closest('[data-download-window]') !== null;
  }

  // Each key runs the palette's command for it, so a key and the palette never differ.
  function onkeydown(e: KeyboardEvent) {
    if (inDownloadWindow(e)) return;
    const command = windowCommand(e);
    if (command === null) return;
    if (command === 'palette') {
      // The palette's own key also closes it.
      e.preventDefault();
      if (!blocked) app.paletteOpen = !app.paletteOpen;
      return;
    }
    // Every other key waits while the palette or a dialog is open.
    if (app.paletteOpen || blocked) return;
    e.preventDefault();
    runKey(app, command);
  }

  function onpaste(e: ClipboardEvent) {
    if (inDownloadWindow(e)) return;
    if (app.paletteOpen || blocked || isEditable(e.target)) return;
    const text = e.clipboardData?.getData('text/plain') ?? '';
    if (!text) return;
    e.preventDefault();
    app.pasteText(text);
  }

  /** A link dragged in from a browser or another app, not one of our own rows. */
  function isLinkDrag(e: DragEvent): boolean {
    const types = e.dataTransfer?.types ?? [];
    return !isDownloadDrag(e) && (types.includes('text/uri-list') || types.includes('text/plain'));
  }

  function ondragover(e: DragEvent) {
    if (blocked || !isLinkDrag(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
  }

  function ondrop(e: DragEvent) {
    if (blocked || !isLinkDrag(e)) return;
    e.preventDefault();
    // `getData` gives an empty string for a type the drag does not carry.
    const links = e.dataTransfer?.getData('text/uri-list') ?? '';
    const text = links === '' ? (e.dataTransfer?.getData('text/plain') ?? '') : links;
    if (text) app.pasteText(text);
  }
</script>

<svelte:window {onkeydown} {ondragover} {ondrop} />
<svelte:document {onpaste} />

<div class="app" class:docked={detail === 'docked'}>
  <Sidebar />
  <main class="main" aria-label={app.screen === 'settings' ? 'Settings' : 'Downloads'}>
    {#if app.screen === 'settings'}
      <Settings />
    {:else}
      <Toolbar />
      <StorageBanner />
      {#if app.loadError}
        <div class="load-error" role="alert">
          <p class="title">{PRODUCT_NAME} cannot reach its download engine.</p>
          <p>{app.loadError}</p>
          <Button variant="primary" onclick={() => app.init()}>Try again</Button>
        </div>
      {:else}
        <div class="body">
          <DownloadList />
          {#if detail === 'overlay'}
            <DetailPanel mode="overlay" />
          {/if}
        </div>
      {/if}
    {/if}
  </main>
  {#if detail === 'docked'}
    <DetailPanel mode="docked" />
  {/if}
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
<Toasts />

<style>
  .app {
    position: relative;
    display: grid;
    grid-template-columns: var(--sidebar-width) minmax(0, 1fr);
    height: 100vh;
  }

  .app.docked {
    grid-template-columns: var(--sidebar-width) minmax(0, 1fr) auto;
  }

  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--surface);
    border-left: 1px solid var(--line);
    /* The toolbar and the list adapt to the room they get, not to the window. */
    container: main / inline-size;
  }

  .body {
    position: relative;
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  .load-error {
    margin: 64px auto;
    max-width: 440px;
    color: var(--ink-2);
  }

  .load-error .title {
    color: var(--ink);
    font-size: var(--text-lg);
    font-weight: 600;
  }
</style>
