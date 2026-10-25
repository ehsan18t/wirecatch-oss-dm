<script lang="ts">
  // One of the queue dialog's cards: a switch that opens its own fields underneath when it is on.
  import type { Snippet } from 'svelte';
  import { Switch } from '@wirecatch/ui';

  let {
    label,
    description,
    checked,
    onchange,
    children,
  }: {
    label: string;
    description: string;
    checked: boolean;
    onchange: (on: boolean) => void;
    children: Snippet;
  } = $props();
</script>

<section class="flex flex-col gap-3 rounded-md border border-line bg-layer p-3">
  <Switch labelSide="start" {label} {description} {checked} {onchange} />
  {#if checked}
    {@render children()}
  {/if}
</section>
