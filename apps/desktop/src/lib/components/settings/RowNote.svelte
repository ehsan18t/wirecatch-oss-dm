<script lang="ts">
  // A line under a setting's row, across it, in the row's padding.
  import type { Snippet } from 'svelte';

  let { children }: { children: Snippet } = $props();
</script>

<p class="px-4 pb-3 text-small text-ink-muted">{@render children()}</p>
