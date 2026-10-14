<script lang="ts">
  import type { Snippet } from 'svelte';
  import { visuallyHidden, type VisuallyHiddenVariants } from './visually-hidden';

  let { focusable = false, children }: VisuallyHiddenVariants & { children: Snippet } = $props();
</script>

<span class={visuallyHidden({ focusable })}>{@render children()}</span>
