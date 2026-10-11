<script lang="ts">
  // The sample app's download windows (D23). A browser tab cannot open real windows that share
  // the sample data, so each one floats inside the page, in the same cascade and with the same
  // rules as the desktop's. Loaded only with the sample backend.
  import type { Backend } from '$lib/backend/types';
  import type { OpenWindow, WindowRegistry } from './registry';
  import MockWindow from './MockWindow.svelte';

  let { backend, registry }: { backend: Backend; registry: WindowRegistry } = $props();

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
    {backend}
    {window}
    front={front === window.label}
    onfocus={() => registry.focus(window.label)}
    onclose={() => registry.gone(window.label)}
    onbecame={(id) => registry.becameDownload(window.label, id)}
  />
{/each}
