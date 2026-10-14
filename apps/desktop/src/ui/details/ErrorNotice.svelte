<script lang="ts">
  // A download's error, worded for a person, with its fix as a button (D3, rule 4), and a plain Try again where the fix is something else. The details and a download's own window each run the fix their own way.
  import type { DownloadSummary, FixAction } from '@wirecatch/protocol';
  import { Button, Callout } from '@wirecatch/ui';
  import { FIX_LABEL } from '$lib/format';

  interface Props {
    row: DownloadSummary;
    onfix: (fix: FixAction) => void;
    onretry: () => void;
    /** Announce it when it appears, as in a window the person is looking at. */
    live?: boolean;
  }

  let { row, onfix, onretry, live = false }: Props = $props();
</script>

{#if row.error}
  {@const fix = row.error.fix}
  <div class="flex flex-col gap-2">
    <Callout
      tone="danger"
      title="This download needs attention"
      live={live ? 'assertive' : undefined}>{row.error.message}</Callout
    >
    <div class="flex flex-wrap gap-2">
      {#if fix}
        <Button size="sm" variant="primary" onclick={() => onfix(fix)}>{FIX_LABEL[fix]}</Button>
      {/if}
      {#if fix !== 'retry'}
        <Button size="sm" onclick={onretry}>Try again</Button>
      {/if}
    </div>
  </div>
{/if}
