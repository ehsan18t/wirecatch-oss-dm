<script lang="ts">
  import { cn } from '@wirecatch/ui';
  import type { Snippet } from 'svelte';

  interface Props {
    title: string;
    note?: string;
    class?: string;
    children: Snippet;
  }

  let { title, note, class: className, children }: Props = $props();
</script>

<div class="flex flex-col gap-3 rounded-lg border border-line bg-raised p-5 shadow-raised">
  <div class="flex items-baseline justify-between gap-4">
    <h3 class="text-ui font-semibold text-ink">{title}</h3>
    {#if note}<p class="text-small text-ink-subtle">{note}</p>{/if}
  </div>
  <div class={cn('flex flex-wrap items-center gap-3', className)}>{@render children()}</div>
</div>
