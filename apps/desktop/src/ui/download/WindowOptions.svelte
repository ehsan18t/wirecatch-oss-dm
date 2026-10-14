<script lang="ts">
  // The two choices a question window offers about the windows themselves (D23, D36.7). Both are app settings, also in Settings and the command palette, so they apply to every later download. "Start downloads without asking" is how a person turns these windows off for files from the browser: the main window then shows a note for each instead.
  import { Checkbox } from '@wirecatch/ui';
  import type { DownloadWindowState } from '$lib/window/download-window.svelte';

  /** `files` adds "Start downloads without asking", which only files from the browser have. */
  let { win, files = false }: { win: DownloadWindowState; files?: boolean } = $props();

  const startWithoutAsking = $derived(win.settings?.extension.ask_before_download === false);
  const closeWhenStarted = $derived(win.settings?.show_progress_windows === false);
</script>

{#if win.settings}
  <div class="flex flex-col gap-3 border-t border-line pt-4">
    {#if files}
      <Checkbox
        checked={startWithoutAsking}
        label="Start downloads without asking"
        description="Files from your browser then start at once, with no window, and the main window shows a note for each. Videos still ask for a quality. You can change this in Settings, Browser."
        onchange={(on: boolean) => void win.setStartWithoutAsking(on)}
      />
    {/if}
    <Checkbox
      checked={closeWhenStarted}
      label="Close this window when downloads start"
      description="Otherwise the window stays and shows the download's progress."
      onchange={(on: boolean) => void win.setCloseWhenStarted(on)}
    />
  </div>
{/if}
