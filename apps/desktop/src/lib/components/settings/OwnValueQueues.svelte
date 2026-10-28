<script lang="ts">
  // Under an app setting a queue can override: how many queues use their own value, each name opening its queue (D72.5).
  import type { Queue } from '@wirecatch/protocol';
  import { Button } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';

  let { queues }: { queues: readonly Queue[] } = $props();

  const app = getApp();
</script>

{#if queues.length > 0}
  <p class="px-4 pb-3 text-small text-ink-muted">
    {queues.length === 1
      ? '1 queue uses its own value:'
      : `${String(queues.length)} queues use their own value:`}
    {#each queues as queue, i (queue.id)}
      <Button
        variant="link"
        size="sm"
        class="text-small"
        onclick={() => app.openDialog({ kind: 'queue', id: queue.id })}
        >{queue.settings.name}</Button
      >{i < queues.length - 1 ? ', ' : '.'}
    {/each}
  </p>
{/if}
