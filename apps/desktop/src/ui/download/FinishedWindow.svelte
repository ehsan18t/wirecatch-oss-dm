<script lang="ts">
  // The download complete window (D52), in the download windows' shell.
  import type { FinishedWindowState } from '$lib/window/finished-window.svelte';
  import FinishedView from './FinishedView.svelte';
  import WindowShell from './WindowShell.svelte';

  let { win }: { win: FinishedWindowState } = $props();
</script>

<WindowShell name={win.name} onclose={() => win.close()}>
  {#if win.loaded}
    <div class="h-full animate-rise-in">
      <FinishedView {win} />
    </div>
  {:else}
    <p class="p-5 text-body text-ink-muted" role="status">Loading.</p>
  {/if}
</WindowShell>
