<script lang="ts">
  // The sample app's download windows (D23). A browser tab cannot open real windows that share the sample data, so each one floats inside the page, in the same cascade and with the same rules as the desktop's. Loaded only with the sample backend.
  import type { DownloadId } from '@wirecatch/protocol';
  import type { OpenWindow, WindowRegistry } from '$lib/sample-windows';
  import type { OpenDownloadWindow } from '$lib/window/download-window.svelte';
  import MockWindow from './MockWindow.svelte';

  let { registry, openWindow }: { registry: WindowRegistry; openWindow: OpenDownloadWindow } =
    $props();

  let windows = $state<readonly OpenWindow[]>([]);
  let front = $state<string | null>(null);

  $effect(() =>
    registry.subscribe((open) => {
      windows = open;
      front = registry.front;
    }),
  );
</script>

{#each windows as window (window.label)}
  <MockWindow
    {openWindow}
    {window}
    front={front === window.label}
    onfocus={() => registry.focus(window.label)}
    onclose={() => registry.gone(window.label)}
    onbecame={(id: DownloadId) => registry.becameDownload(window.label, id)}
  />
{/each}
