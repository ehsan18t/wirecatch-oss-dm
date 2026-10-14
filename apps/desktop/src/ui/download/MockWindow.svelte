<script lang="ts">
  // One of the sample app's download windows, floating in the page at its place in the cascade, as wide as a real one and as tall as its content, within the page.
  import { PRODUCT_NAME } from '@wirecatch/product';
  import {
    DOWNLOAD_WINDOW_CASCADE_STEP,
    DOWNLOAD_WINDOW_MIN_HEIGHT,
    DOWNLOAD_WINDOW_WIDTH,
    type DownloadId,
  } from '@wirecatch/protocol';
  import { onMount } from 'svelte';
  import { cn } from '@wirecatch/ui';
  import type { OpenWindow } from '$lib/sample-windows';
  import type { OpenDownloadWindow } from '$lib/window/download-window.svelte';
  import { setFitWindow } from '$lib/window/fit';
  import DownloadWindow from './DownloadWindow.svelte';

  /** Where the first sample window sits, down and right of the page's top left corner, in CSS pixels. */
  const CASCADE_ORIGIN = 48;
  /** The room a window leaves at the page's right and bottom edges, in CSS pixels. */
  const EDGE = 16;
  /** The window's border, both sides together, in CSS pixels. */
  const FRAME = 2;

  interface Props {
    openWindow: OpenDownloadWindow;
    window: OpenWindow;
    front: boolean;
    onfocus: () => void;
    onclose: () => void;
    onbecame: (id: DownloadId) => void;
  }

  let { openWindow, window: open, front, onfocus, onclose, onbecame }: Props = $props();

  const offset = $derived(CASCADE_ORIGIN + open.slot * DOWNLOAD_WINDOW_CASCADE_STEP);
  // Made once for the window's life; what it shows comes from the registry only at the start.
  const win = (() => {
    const shows = open.shows;
    return openWindow(shows.kind === 'question' ? { question: shows.id } : { download: shows.id }, {
      close: () => onclose(),
      becameDownload: (id) => onbecame(id),
    });
  })();

  let pageWidth = $state(0);
  let pageHeight = $state(0);
  /** The height the window's content asked for, once it has. */
  let wanted = $state<number | null>(null);
  setFitWindow((height) => (wanted = height));

  const width = $derived(Math.min(DOWNLOAD_WINDOW_WIDTH, pageWidth - offset - EDGE - FRAME));
  const height = $derived.by(() => {
    const room = pageHeight - offset - EDGE - FRAME;
    return Math.min(room, Math.max(DOWNLOAD_WINDOW_MIN_HEIGHT, wanted ?? room));
  });

  onMount(() => {
    void win.init();
    return () => win.dispose();
  });
</script>

<svelte:window bind:innerWidth={pageWidth} bind:innerHeight={pageHeight} />

<!-- A window of its own in the real app; here a labelled region of the page. Its border lies outside the size, as a real window's frame does, so the content gets all the height it asked for. -->
<section
  aria-label="{PRODUCT_NAME} download window {open.slot + 1}"
  data-download-window
  data-label={open.label}
  class={cn(
    'fixed box-content flex flex-col overflow-hidden rounded-lg border border-line-strong bg-base shadow-dialog',
    front ? 'z-window-front' : 'z-window',
  )}
  style:left="{offset}px"
  style:top="{offset}px"
  style:width="{width}px"
  style:height="{height}px"
  onpointerdown={onfocus}
  onfocusin={onfocus}
>
  <DownloadWindow {win} />
</section>
