<script lang="ts">
  // A group of choices folded into one line that says what they are set to ("Subtitles: English · Chapters"), opened with a click when the person wants to change them. It keeps a form short when most people take the defaults.
  import ChevronRight from '~icons/lucide/chevron-right';
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import { FOCUS_RING } from '../recipes/control';
  import Icon from './Icon.svelte';

  interface Props {
    /** What the group is: "Subtitles", "Save to". */
    label: string;
    /** What it is set to now, shown beside the label while folded. */
    summary?: string | undefined;
    open?: boolean;
    class?: string | undefined;
    children: Snippet;
  }

  let { label, summary, open = $bindable(false), class: className, children }: Props = $props();

  const id = $props.id();
</script>

<div class={cn('flex min-w-0 flex-col', className)}>
  <button
    type="button"
    aria-expanded={open}
    aria-controls={id}
    class={cn(
      '-mx-2 flex min-h-control min-w-0 items-center gap-2 rounded-sm px-2 text-left text-ui transition-colors duration-fast hover:bg-wash',
      FOCUS_RING,
    )}
    onclick={() => (open = !open)}
  >
    <Icon
      icon={ChevronRight}
      size="sm"
      class={cn('shrink-0 text-ink-subtle transition-transform duration-fast', open && 'rotate-90')}
    />
    <span class="shrink-0 font-medium text-ink">{label}</span>
    {#if summary && !open}
      <span class="min-w-0 truncate text-ink-muted">{summary}</span>
    {/if}
  </button>
  <!-- `hidden` rather than a class: folded content is out of the page and the accessibility tree wherever styles are missing too. -->
  <div {id} hidden={!open} class="flex flex-col gap-3 pt-2 pb-1">
    {@render children()}
  </div>
</div>
