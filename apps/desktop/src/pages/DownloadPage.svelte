<script lang="ts">
  // A download window (D23), opened by the desktop shell at `DOWNLOAD_WINDOW_PAGE` with the id of its question (`QUESTION_PARAMETER`) or of its download (`DOWNLOAD_PARAMETER`). It loads only its own question or download, never the whole list, because every window is a webview of its own.
  import { PRODUCT_NAME } from '@wirecatch/product';
  import {
    DOWNLOAD_PARAMETER,
    isDownloadId,
    isQuestionId,
    QUESTION_PARAMETER,
  } from '@wirecatch/protocol';
  import { onMount } from 'svelte';
  import { createDownloadWindowBackend, inTauri, type DownloadWindowBackend } from '$lib/backend';
  import { Appearance, followLook } from '$lib/state/appearance.svelte';
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
    const question = number(QUESTION_PARAMETER);
    if (question !== null && isQuestionId(question)) return { question };
    const download = number(DOWNLOAD_PARAMETER);
    return download !== null && isDownloadId(download) ? { download } : null;
  }

  let win = $state<DownloadWindowState | null>(null);
  let problem = $state<string | null>(null);
  let backend: DownloadWindowBackend | null = null;
  const appearance = new Appearance();

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
    const shell = createDownloadWindowBackend();
    backend = shell;
    const state = new DownloadWindowState(
      {
        backend: () => shell,
        close: () => void shell.closeDownloadWindow().catch(() => window.close()),
      },
      wanted,
    );
    win = state;
    void state.init();
    const following = appearance.connect(shell);
    return () => {
      state.dispose();
      void following.then((stop) => stop());
    };
  });

  followLook(appearance, () => win?.settings);
</script>

<svelte:head><title>{win?.name ?? PRODUCT_NAME} download</title></svelte:head>

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
