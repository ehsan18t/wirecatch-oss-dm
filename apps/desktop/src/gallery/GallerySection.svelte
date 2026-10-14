<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    id: string;
    title: string;
    description: string;
    children: Snippet;
  }

  let { id, title, description, children }: Props = $props();
</script>

<section {id} aria-labelledby="{id}-title" class="flex scroll-mt-20 flex-col gap-6">
  <header class="flex flex-col gap-1">
    <h2 id="{id}-title" class="font-display text-title font-semibold tracking-display text-ink">
      {title}
    </h2>
    <p class="max-w-prose text-body text-ink-muted">{description}</p>
  </header>
  {@render children()}
</section>
