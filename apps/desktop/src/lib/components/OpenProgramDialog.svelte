<script lang="ts">
  import type { DownloadId } from '@wirecatch/protocol';
  import { fileType } from '$lib/filetypes';
  import { hostOf } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import Dialog from './Dialog.svelte';

  let { id }: { id: DownloadId } = $props();
  const app = getApp();

  const row = $derived(app.downloads.get(id));
  const site = $derived(row ? hostOf(row.url) : 'where it came from');
  // The type the system goes by, so a name dressed up as something else is called what it is.
  const type = $derived(row ? fileType(row.file_name) : null);
  // Isolated, so characters in the name cannot reorder the text around it.
  const name = $derived(`⁨${row?.file_name ?? 'this file'}⁩`);

  async function showInFolder() {
    app.closeDialog();
    await app.openFolder(id);
  }
</script>

<Dialog title={`Open "${name}"?`} onclose={() => app.closeDialog()} width={460}>
  <p class="text">
    {#if type}
      This is a <strong>.{type}</strong> file, a type that can run code.
    {:else}
      <bdi>{row?.file_name ?? 'This file'}</bdi> has no file type, so it may run code.
    {/if}
    Only open it if you trust <strong>{site}</strong>. Code can change your computer, and a file
    from an unknown site can contain anything.
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
