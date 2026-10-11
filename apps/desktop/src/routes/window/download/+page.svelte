<script lang="ts">
  // A download window (D23), opened by the desktop shell as `window/download?question=<id>` or
  // `window/download?download=<id>`. It loads only its own question or download, never the
  // whole list, because every window is a webview of its own.
  import { onMount } from 'svelte';
  import { inTauri } from '$lib/backend';
  import { TauriBackend } from '$lib/backend/tauri';
  import DownloadWindow from '$lib/window/DownloadWindow.svelte';
  import { DownloadWindowState, type Target } from '$lib/window/download-window.svelte';
  import { setFitWindow } from '$lib/window/fit';

  /** What the address asks for, or `null` when it names nothing usable. */
  function target(search: string): Target | null {
    const params = new URLSearchParams(search);
    const number = (key: string) => {
      const value = params.get(key);
      return value !== null && /^\d+$/.test(value) ? Number(value) : null;
    };
    const question = number('question');
    if (question !== null) return { question };
    const download = number('download');
    return download === null ? null : { download };
  }

  let win = $state<DownloadWindowState | null>(null);
  let problem = $state<string | null>(null);
  let backend: TauriBackend | null = null;

  // The window is only as tall as the view it shows; a failure leaves it as it is.
  setFitWindow((height) => void backend?.fitDownloadWindow(height).catch(() => undefined));

  onMount(() => {
    const wanted = target(window.location.search);
    if (!inTauri()) {
      problem =
        'This page is one of Wirecatch’s download windows and only works inside the app. In the sample app, download windows open inside the main page.';
      return;
    }
    if (!wanted) {
      problem = 'This window does not say which download it is for. Close it and try again.';
      return;
    }
    const shell = new TauriBackend();
    backend = shell;
    const state = new DownloadWindowState(shell, wanted, {
      close: () => void shell.closeDownloadWindow().catch(() => window.close()),
    });
    win = state;
    void state.init();
    return () => state.dispose();
  });

  // Follow the theme setting, like the main window.
  $effect(() => {
    const theme = win?.settings?.theme ?? 'system';
    const root = document.documentElement;
    if (theme === 'system') delete root.dataset.theme;
    else root.dataset.theme = theme;
  });
</script>

<svelte:head><title>Wirecatch download</title></svelte:head>

<main class="window" aria-label="Download">
  {#if win}
    <DownloadWindow {win} />
  {:else if problem}
    <p class="problem" role="alert">{problem}</p>
  {/if}
</main>

<style>
  .window {
    height: 100vh;
    background: var(--surface);
  }

  .problem {
    margin: 0;
    padding: 20px;
    line-height: 1.5;
  }
</style>
