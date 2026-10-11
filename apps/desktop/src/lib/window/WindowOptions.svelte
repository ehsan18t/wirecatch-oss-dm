<script lang="ts">
  // The two choices a question window offers about windows themselves (D23). Both are app
  // settings, also in Settings and the command palette, so they apply to every later download.
  import type { DownloadWindowState } from './download-window.svelte';

  /** `files` adds "Start downloads without asking", which only browser files have. */
  let { win, files = false }: { win: DownloadWindowState; files?: boolean } = $props();
  const uid = $props.id();

  const startWithoutAsking = $derived(win.settings?.extension.ask_before_download === false);
  const closeWhenStarted = $derived(win.settings?.show_progress_windows === false);
</script>

{#if win.settings}
  <div class="options">
    {#if files}
      <label class="option">
        <input
          type="checkbox"
          checked={startWithoutAsking}
          aria-describedby="{uid}-ask"
          onchange={(e) => win.setStartWithoutAsking(e.currentTarget.checked)}
        />
        <span>Start downloads without asking</span>
      </label>
      <p id="{uid}-ask" class="help">
        Browser files then start at once, with no window. Videos still ask for a quality. You can
        turn this back on in Settings, Browser.
      </p>
    {/if}
    <label class="option">
      <input
        type="checkbox"
        checked={closeWhenStarted}
        aria-describedby="{uid}-close"
        onchange={(e) => win.setCloseWhenStarted(e.currentTarget.checked)}
      />
      <span>Close this window when downloads start</span>
    </label>
    <p id="{uid}-close" class="help">
      Otherwise the window stays and shows the download's progress.
    </p>
  </div>
{/if}

<style>
  .options {
    display: grid;
    gap: 2px;
    margin-top: 4px;
    padding-top: 12px;
    border-top: 1px solid var(--line);
  }

  .option {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .option input {
    width: 16px;
    height: 16px;
    margin: 0;
    accent-color: var(--accent);
  }

  .help {
    margin: 0 0 6px 24px;
    color: var(--ink-2);
    font-size: var(--text-xs);
    line-height: 1.45;
  }
</style>
