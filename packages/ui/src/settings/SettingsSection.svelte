<script lang="ts">
  // A group of settings under a heading, on one raised card with a hairline between its rows. The app's settings page and the extension's options page both build on it, so a setting looks the same wherever it is changed.
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';

  interface Props {
    title: string;
    /** A line under the title, for what the whole group is about. */
    description?: string | undefined;
    /** The heading the title takes in the page's outline. */
    heading?: 'h2' | 'h3';
    class?: string | undefined;
    children: Snippet;
  }

  let { title, description, heading = 'h2', class: className, children }: Props = $props();

  const id = $props.id();
  const titleId = `${id}-title`;
</script>

<section aria-labelledby={titleId} class={cn('flex flex-col gap-2', className)}>
  <div class="flex flex-col gap-0.5 px-1">
    <svelte:element this={heading} id={titleId} class="text-ui font-semibold text-ink">
      {title}
    </svelte:element>
    {#if description}
      <p class="text-small text-ink-muted">{description}</p>
    {/if}
  </div>
  <div
    class="flex flex-col divide-y divide-line rounded-lg border border-line bg-raised shadow-raised forced-colors:border-sys-text"
  >
    {@render children()}
  </div>
</section>
