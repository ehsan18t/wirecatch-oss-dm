<script lang="ts">
  // A shortcut's keys as caps side by side, the way the keyboard shows them, with no plus between.
  import { cn } from '../lib/cn';
  import { keyGlyph } from '../lib/keys';

  interface Props {
    /** The keys of one shortcut, pressed together: `['Ctrl', 'Shift', 'V']`. */
    keys: readonly string[];
    class?: string | undefined;
  }

  let { keys, class: className }: Props = $props();
</script>

<kbd class={cn('inline-flex items-center gap-0.5 font-sans', className)}>
  {#each keys as key, index (index)}
    <!-- The plus is read, not shown: a screen reader and a copy say "Alt+Shift+D", where the caps alone run together. -->
    {#if index > 0}<span class="sr-only">+</span>{/if}
    <kbd
      class="inline-flex h-5 min-w-5 items-center justify-center rounded-xs border border-line bg-wash px-1.5 font-sans text-caption font-medium text-ink-muted"
      >{keyGlyph(key)}</kbd
    >
  {/each}
</kbd>
