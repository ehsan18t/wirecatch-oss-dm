<script lang="ts">
  // One sample download window, floating in the page at its place in the cascade.
  import X from '@lucide/svelte/icons/x';
  import { DOWNLOAD_WINDOW_CASCADE_STEP, type DownloadId } from '@wirecatch/protocol';
  import { onMount } from 'svelte';
  import type { Backend } from '$lib/backend';
  import IconButton from '$lib/components/IconButton.svelte';
  import DownloadWindow from './DownloadWindow.svelte';
  import { DownloadWindowState } from './download-window.svelte';
  import type { OpenWindow } from './registry';

  /** Where the first sample window sits, down and right of the page's top left corner. */
  const CASCADE_ORIGIN = 48;

  let {
    backend,
    window: open,
    front,
    onfocus,
    onclose,
    onbecame,
  }: {
    backend: Backend;
    window: OpenWindow;
    front: boolean;
    onfocus: () => void;
    onclose: () => void;
    onbecame: (id: DownloadId) => void;
  } = $props();

  const uid = $props.id();
  const offset = $derived(CASCADE_ORIGIN + open.slot * DOWNLOAD_WINDOW_CASCADE_STEP);
  // Made once for the window's life; what it shows comes from the registry only at the start.
  const win = (() => {
    const shows = open.shows;
    return new DownloadWindowState(
      backend,
      shows.kind === 'question' ? { question: shows.id } : { download: shows.id },
      { close: () => onclose(), becameDownload: (id) => onbecame(id) },
    );
  })();

  onMount(() => {
    void win.init();
    return () => win.dispose();
  });
</script>

<!-- A window of its own in the real app; here a labelled region of the page. -->
<div
  class="mock-window"
  class:front
  data-download-window
  role="region"
  aria-labelledby="{uid}-bar"
  data-label={open.label}
  style:left="{offset}px"
  style:top="{offset}px"
  onpointerdown={onfocus}
  onfocusin={onfocus}
>
  <div class="bar">
    <span id="{uid}-bar">Wirecatch download window {open.slot + 1}</span>
    <IconButton icon={X} label="Close this window" onclick={() => win.close()} />
  </div>
  <div class="content">
    <DownloadWindow {win} />
  </div>
</div>

<style>
  .mock-window {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    width: min(460px, calc(100vw - 32px));
    height: min(600px, calc(100vh - 64px));
    overflow: hidden;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-l);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .mock-window.front {
    z-index: 41;
  }

  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 6px 4px 14px;
    border-bottom: 1px solid var(--line);
    background: var(--surface-2);
    color: var(--ink-2);
    font-size: var(--text-xs);
  }

  .content {
    flex: 1;
    min-height: 0;
  }
</style>
