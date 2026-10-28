<script lang="ts">
  // Under an app setting a queue can override: how many queues use their own value, each name opening its queue (D72.5).
  import type { Queue } from '@wirecatch/protocol';
  import { cn, FOCUS_RING } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';

  let { queues }: { queues: readonly Queue[] } = $props();

  const app = getApp();
</script>

{#if queues.length > 0}
  <!-- Plain buttons, not Button: its sibling for a disabled reason would put a space before each comma. -->
  <p class="px-4 pb-3 text-small text-ink-muted">
    {queues.length === 1
      ? '1 queue uses its own value:'
      : `${String(queues.length)} queues use their own value:`}
    {#each queues as queue, i (queue.id)}<button
        type="button"
        class={cn(
          'rounded-xs text-accent underline decoration-from-font underline-offset-2 hover:decoration-2',
          FOCUS_RING,
        )}
        onclick={() => app.openDialog({ kind: 'queue', id: queue.id })}
        >{queue.settings.name}</button
      >{i < queues.length - 1 ? ', ' : '.'}{/each}
  </p>
{/if}
