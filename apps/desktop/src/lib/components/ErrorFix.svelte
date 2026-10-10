<script lang="ts">
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import type { DownloadSummary, FixAction } from '@wirecatch/protocol';
  import { getApp } from '$lib/state/app.svelte';

  let { row }: { row: DownloadSummary } = $props();
  const app = getApp();

  const FIX_LABEL: Record<FixAction, string> = {
    retry: 'Try again',
    refresh_link: 'Get a fresh link',
    choose_folder: 'Choose another folder',
    free_disk_space: 'Open the folder to free space',
  };
</script>

{#if row.error}
  <div class="error" role="alert">
    <p class="message">
      <CircleAlert size={16} aria-hidden="true" />
      <span>{row.error.message}</span>
    </p>
    <div class="actions">
      {#if row.error.fix}
        {@const fix = row.error.fix}
        <button class="btn primary" onclick={() => app.fix(row.id, fix)}>{FIX_LABEL[fix]}</button>
      {/if}
      {#if row.error.fix !== 'retry'}
        <button class="btn" onclick={() => app.retry([row.id])}>Try again</button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .error {
    padding: 12px;
    border: 1px solid color-mix(in srgb, var(--danger) 35%, transparent);
    border-radius: var(--radius-m);
    background: var(--danger-soft);
  }

  .message {
    display: flex;
    gap: 8px;
    margin: 0;
    color: var(--ink);
    line-height: 1.45;
  }

  .message :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--danger);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 10px;
  }
</style>
