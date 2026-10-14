<script lang="ts">
  // A download's error, worded for a person, with its fix as a button (D3, rule 4), and a plain Try again where the fix is something else.
  import type { DownloadSummary } from '@wirecatch/protocol';
  import { Button, Callout } from '@wirecatch/ui';
  import { FIX_LABEL } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';

  let { row }: { row: DownloadSummary } = $props();

  const app = getApp();
</script>

{#if row.error}
  {@const fix = row.error.fix}
  <div class="flex flex-col gap-2">
    <Callout tone="danger" title="This download needs attention">{row.error.message}</Callout>
    <div class="flex flex-wrap gap-2">
      {#if fix}
        <Button size="sm" variant="primary" onclick={() => app.actions.fix(row.id, fix)}
          >{FIX_LABEL[fix]}</Button
        >
      {/if}
      {#if fix !== 'retry'}
        <Button size="sm" onclick={() => app.actions.retry([row.id])}>Try again</Button>
      {/if}
    </div>
  </div>
{/if}
