<script lang="ts">
  import { focusSearch } from '$lib/commands';
  import { getApp } from '$lib/state/app.svelte';
  import CommandPalette from './CommandPalette.svelte';
  import ConfirmCapture from './ConfirmCapture.svelte';
  import DetailPanel from './DetailPanel.svelte';
  import Dialogs from './Dialogs.svelte';
  import DownloadList from './DownloadList.svelte';
  import Settings from './Settings.svelte';
  import Sidebar from './Sidebar.svelte';
  import Toasts from './Toasts.svelte';
  import Toolbar from './Toolbar.svelte';

  const app = getApp();

  // Apply the theme setting. "System" leaves it to the operating system's preference.
  $effect(() => {
    const theme = app.settings?.theme ?? 'system';
    const root = document.documentElement;
    if (theme === 'system') delete root.dataset.theme;
    else root.dataset.theme = theme;
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

  const blocked = $derived(app.dialog !== null || app.pending.length > 0);

  function onkeydown(e: KeyboardEvent) {
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
    if (app.paletteOpen || blocked || isEditable(e.target)) return;
    const text = e.clipboardData?.getData('text/plain') ?? '';
    if (!text) return;
    e.preventDefault();
    app.pasteText(text);
  }
</script>

<svelte:window {onkeydown} />
<svelte:document {onpaste} />

<div class="app">
  <Sidebar />
  <main class="main" aria-label={app.screen === 'settings' ? 'Settings' : 'Downloads'}>
    {#if app.screen === 'settings'}
      <Settings />
    {:else}
      <Toolbar />
      {#if app.loadError}
        <div class="load-error" role="alert">
          <p class="title">Wirecatch cannot reach its download engine.</p>
          <p>{app.loadError}</p>
          <button class="btn primary" onclick={() => app.init()}>Try again</button>
        </div>
      {:else}
        <DownloadList />
      {/if}
    {/if}
  </main>
  {#if app.screen === 'downloads'}
    <DetailPanel />
  {/if}
</div>

<Dialogs />
<ConfirmCapture />
<CommandPalette />
<Toasts />

<style>
  .app {
    display: grid;
    grid-template-columns: 236px minmax(0, 1fr) auto;
    height: 100vh;
  }

  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--surface);
    border-left: 1px solid var(--line);
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
