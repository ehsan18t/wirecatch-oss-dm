<script lang="ts">
  // A download's error, worded for a person, with its fix as a button (D3, rule 4). Used by the
  // detail panel and by download windows, which each run the fix their own way.
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import type { DownloadSummary, FixAction } from '@wirecatch/protocol';
  import { FIX_LABEL } from '$lib/format';
  import Button from './Button.svelte';
  import Icon from './Icon.svelte';

  let {
    row,
    onfix,
    onretry,
  }: {
    row: DownloadSummary;
    onfix: (fix: FixAction) => unknown;
    onretry: () => unknown;
  } = $props();
</script>

{#if row.error}
  <div class="error" role="alert">
    <p class="message">
      <Icon icon={CircleAlert} size="md" />
      <span>{row.error.message}</span>
    </p>
    <div class="actions">
      {#if row.error.fix}
        {@const fix = row.error.fix}
        <Button variant="primary" onclick={() => onfix(fix)}>{FIX_LABEL[fix]}</Button>
      {/if}
      {#if row.error.fix !== 'retry'}
        <Button onclick={() => onretry()}>Try again</Button>
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
