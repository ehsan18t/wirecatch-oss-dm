<script lang="ts">
  // A video being read for its qualities, which can take a few seconds. The quality list replaces this in the same window.
  import { PRODUCT_NAME } from '@wirecatch/product';
  import { Button, Spinner } from '@wirecatch/ui';
  import { hostOf } from '$lib/format';
  import type { DownloadWindowState } from '$lib/window/download-window.svelte';
  import WindowFrame from './WindowFrame.svelte';

  let { win, url }: { win: DownloadWindowState; url: string } = $props();
</script>

<WindowFrame title="Reading the video" error={win.error}>
  <div class="flex items-start gap-3" role="status">
    <Spinner size="lg" class="mt-0.5" />
    <p class="text-body text-ink">
      {PRODUCT_NAME} is finding the qualities on {hostOf(url)}. This takes a few seconds, then you
      can pick one here.
    </p>
  </div>

  {#snippet footer()}
    <Button variant="ghost" onclick={() => void win.discard()}>Cancel</Button>
    <Button data-autofocus onclick={() => win.close()}>Hide</Button>
  {/snippet}
</WindowFrame>
