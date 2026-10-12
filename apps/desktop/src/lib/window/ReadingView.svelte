<script lang="ts">
  // A video being read for its qualities, which can take a few seconds. The quality list
  // replaces this in the same window.
  import Button from '$lib/components/Button.svelte';
  import { hostOf } from '$lib/format';
  import type { DownloadWindowState } from './download-window.svelte';
  import WindowFrame from './WindowFrame.svelte';

  let { win, url }: { win: DownloadWindowState; url: string } = $props();
</script>

<WindowFrame title="Reading the video">
  <div class="reading" role="status">
    <span class="spinner" aria-hidden="true"></span>
    <p>
      Wirecatch is finding the qualities on {hostOf(url)}. This takes a few seconds, then you can
      pick one here.
    </p>
  </div>
  {#if win.error}<p class="error" role="alert">{win.error}</p>{/if}

  {#snippet footer()}
    <Button onclick={() => win.discard()}>Cancel</Button>
    <Button data-autofocus onclick={() => win.close()}>Hide</Button>
  {/snippet}
</WindowFrame>

<style>
  .reading {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }

  p {
    margin: 0;
    line-height: 1.5;
  }

  .spinner {
    flex: none;
    width: 16px;
    height: 16px;
    margin-top: 2px;
    border: 2px solid var(--wire-track);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.9s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation: none;
    }
  }

  .error {
    margin-top: 10px;
    color: var(--danger);
    font-size: var(--text-sm);
  }
</style>
