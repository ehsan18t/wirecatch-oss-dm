<script lang="ts">
  // Wirecatch's mark: a signal running along a wire, in the accent. Decoration beside the product's name, so it is hidden from screen readers.
  import { cn } from '../lib/cn';
  import { ICON_SIZES, type IconSize } from '../primitives/icon';

  interface Props {
    size?: IconSize;
    class?: string | undefined;
  }

  let { size = 'md', class: className }: Props = $props();
</script>

<svg
  class={cn('shrink-0 text-accent', ICON_SIZES[size], className)}
  viewBox="0 0 20 12"
  aria-hidden="true"
  focusable="false"
>
  <path
    d="M1 6h6l2-4 2 8 2-4h6"
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    stroke-linejoin="round"
  />
</svg>
