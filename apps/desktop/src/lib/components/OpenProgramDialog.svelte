<script lang="ts">
  import type { DownloadId } from '@wirecatch/protocol';
  import { hostOf } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import Dialog from './Dialog.svelte';

  let { id }: { id: DownloadId } = $props();
  const app = getApp();

  const row = $derived(app.downloads.get(id));
  const site = $derived(row ? hostOf(row.url) : 'where it came from');

  async function showInFolder() {
    app.closeDialog();
    await app.openFolder(id);
  }
</script>

<Dialog
  title={`Open "${row?.file_name ?? 'this file'}"?`}
  onclose={() => app.closeDialog()}
  width={460}
>
  <p class="text">
    This file type can run code. Only open it if you trust <strong>{site}</strong>. Code can change
    your computer, and a file from an unknown site can contain anything.
  </p>

  {#snippet footer()}
    <button class="btn" onclick={() => app.openProgram(id)}>Open</button>
    <button class="btn primary" data-autofocus onclick={showInFolder}>Show in folder</button>
  {/snippet}
</Dialog>

<style>
  .text {
    margin: 0;
    color: var(--ink-2);
    line-height: 1.5;
  }

  strong {
    color: var(--ink);
  }
</style>
