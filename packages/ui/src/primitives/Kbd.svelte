<script lang="ts">
  import { cn } from '../lib/cn';

  interface Props {
    /** The keys of one shortcut, pressed together: `['Ctrl', 'Shift', 'V']`. */
    keys: readonly string[];
    class?: string | undefined;
  }

  let { keys, class: className }: Props = $props();
</script>

<kbd class={cn('inline-flex items-center gap-0.5 font-sans', className)}>
  {#each keys as key, index (index)}
    {#if index > 0}<span class="text-caption text-ink-subtle" aria-hidden="true">+</span>{/if}
    <kbd
      class="inline-flex h-5 min-w-5 items-center justify-center rounded-xs border font-sans border-line border-b-line-strong/quiet bg-raised px-1.5 text-caption font-medium text-ink-muted shadow-control"
      >{key}</kbd
    >
  {/each}
</kbd>
