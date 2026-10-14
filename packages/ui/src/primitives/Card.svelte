<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLAttributes } from 'svelte/elements';
  import { card, type CardVariants } from '../recipes/surface';

  interface Props extends Omit<HTMLAttributes<HTMLElement>, 'class' | 'children'>, CardVariants {
    /** The element: a `section` (with a heading inside) for a card that is a part of the page. */
    as?: 'div' | 'section' | 'article';
    class?: string | undefined;
    children: Snippet;
  }

  let {
    as = 'div',
    variant,
    padding,
    radius,
    interactive,
    class: className,
    children,
    ...rest
  }: Props = $props();
</script>

<svelte:element
  this={as}
  class={card({ variant, padding, radius, interactive, class: className })}
  {...rest}
>
  {@render children()}
</svelte:element>
