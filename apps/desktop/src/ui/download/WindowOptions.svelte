<script lang="ts">
  // The choices a question window offers about the windows themselves (D23, D36.7, D63). Both are app settings, also in Settings and the command palette, so they apply to every later download. "Don't ask again" is how a person turns these windows off for files from the browser (each then starts with a brief system notification instead); the other is worded as Settings words it, so it reads the same way round in both places. A window may add its own first (the quality window's "every video", D50).
  import type { Snippet } from 'svelte';
  import { Checkbox } from '@wirecatch/ui';
  import type { DownloadWindowState } from '$lib/window/download-window.svelte';

  interface Props {
    win: DownloadWindowState;
    /** Adds "Don't ask again", which only files from the browser have. */
    files?: boolean;
    /** The window's own choices, first. */
    children?: Snippet;
  }

  let { win, files = false, children }: Props = $props();

  const startWithoutAsking = $derived(win.settings?.extension.ask_before_download === false);
  const keepOpen = $derived(win.settings?.show_progress_windows === true);
</script>

{#if win.settings}
  <div class="flex flex-col gap-3 border-t border-line pt-4">
    {@render children?.()}
    {#if files}
      <Checkbox
        checked={startWithoutAsking}
        label="Don’t ask again for files from the browser"
        description="They start at once; videos follow Settings, Media. Turn it back on in Settings, Browser."
        onchange={(on: boolean) => void win.setStartWithoutAsking(on)}
      />
    {/if}
    <Checkbox
      checked={keepOpen}
      label="Keep the window open to show progress"
      onchange={(on: boolean) => void win.setKeepOpen(on)}
    />
  </div>
{/if}
