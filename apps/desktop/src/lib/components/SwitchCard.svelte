<script lang="ts">
  // One of the queue dialog's cards, or a row of a grouped card (`flush`): a switch that opens its own fields underneath when it is on.
  import type { Snippet } from 'svelte';
  import { Switch } from '@wirecatch/ui';

  let {
    label,
    description,
    checked,
    onchange,
    flush = false,
    children,
  }: {
    label: string;
    description: string;
    checked: boolean;
    onchange: (on: boolean) => void;
    flush?: boolean;
    children: Snippet;
  } = $props();
</script>

<section
  class={flush
    ? 'flex flex-col gap-3 p-3'
    : 'flex flex-col gap-3 rounded-md border border-line bg-layer p-3'}
>
  <Switch labelSide="start" {label} {description} {checked} {onchange} />
  {#if checked}
    {@render children()}
  {/if}
</section>
