<script lang="ts">
  import { onMount } from 'svelte';
  import { focusSearch } from '$lib/commands';
  import { isDownloadDrag } from '$lib/dnd';
  import { getApp } from '$lib/state/app.svelte';
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

  /**
   * Below this width the detail panel floats over the list instead of taking a column. It docks
   * whenever the list beside it still has room for Name, Size, Done and Status: sidebar (236)
   * plus panel (316) plus 600 for the list, which then shows its narrowest set of columns.
   */
  const DOCK_WIDTH = 236 + 316 + 600;

  onMount(() => {
    const query = window.matchMedia(`(min-width: ${DOCK_WIDTH}px)`);
    const update = () => (app.wide = query.matches);
    update();
    query.addEventListener('change', update);
    return () => query.removeEventListener('change', update);
  });

  // Apply the theme setting. "System" leaves it to the operating system's preference.
  $effect(() => {
    const theme = app.settings?.theme ?? 'system';
    const root = document.documentElement;
    if (theme === 'system') delete root.dataset.theme;
    else root.dataset.theme = theme;
  });

  const detail = $derived.by((): 'docked' | 'overlay' | null => {
    if (app.screen !== 'downloads') return null;
    if (app.wide) return app.detailsOpen ? 'docked' : null;
    return app.selection.size > 0 ? 'overlay' : null;
  });

  function isEditable(el: EventTarget | null): boolean {
    if (!(el instanceof HTMLElement)) return false;
    if (el.isContentEditable) return true;
    if (el instanceof HTMLTextAreaElement) return true;
    if (el instanceof HTMLInputElement) {
      return !['checkbox', 'radio', 'button', 'submit', 'range'].includes(el.type);
    }
    return el instanceof HTMLSelectElement;
  }

  // A visible dialog takes the keyboard. Questions have their own windows (D23).
  const blocked = $derived(app.dialog !== null);

  /** A key typed in one of the sample app's download windows, which float in this page. */
  function inDownloadWindow(e: Event): boolean {
    return e.target instanceof Element && e.target.closest('[data-download-window]') !== null;
  }

  function onkeydown(e: KeyboardEvent) {
    if (inDownloadWindow(e)) return;
    const ctrl = e.ctrlKey || e.metaKey;
    if (ctrl && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      if (!blocked) app.paletteOpen = !app.paletteOpen;
      return;
    }
    if (app.paletteOpen || blocked) return;
    if (ctrl && e.key.toLowerCase() === 'n') {
      e.preventDefault();
      app.openDialog({ kind: 'add', urls: [] });
    } else if (ctrl && e.key === ',') {
      e.preventDefault();
      app.openSettings();
    } else if (ctrl && e.key.toLowerCase() === 'f') {
      e.preventDefault();
      if (app.screen === 'settings') document.getElementById('settings-search')?.focus();
      else focusSearch();
    } else if (e.key === 'F1') {
      e.preventDefault();
      app.openDialog({ kind: 'shortcuts' });
    } else if (e.key === 'Escape' && !isEditable(e.target)) {
      if (app.screen === 'settings') app.screen = 'downloads';
      else app.clearSelection();
    }
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
    const text =
      e.dataTransfer?.getData('text/uri-list') || e.dataTransfer?.getData('text/plain') || '';
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
          <p class="title">Wirecatch cannot reach its download engine.</p>
          <p>{app.loadError}</p>
          <button class="btn primary" onclick={() => app.init()}>Try again</button>
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
{#if app.sampleData && app.backend.sampleData}
  {@const sample = app.backend.sampleData}
  <!-- The sample app's download windows float in the page; the real app opens real windows. -->
  {#await import('$lib/window/MockWindows.svelte') then { default: MockWindows }}
    <MockWindows backend={app.backend} registry={sample.windows} />
  {/await}
{/if}
<CommandPalette />
<Toasts />

<style>
  .app {
    position: relative;
    display: grid;
    grid-template-columns: 236px minmax(0, 1fr);
    height: 100vh;
  }

  .app.docked {
    grid-template-columns: 236px minmax(0, 1fr) auto;
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
