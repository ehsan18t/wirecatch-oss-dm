<script lang="ts">
  // Wirecatch's mark: a "W" whose two points land on a bar, in the accent (D108). The desktop app and the icons draw it from the same mark.json. Decoration beside the product's name, so it is hidden from screen readers.
  import { cn } from '../lib/cn';
  import mark from '../mark.json';
  import { ICON_SIZES, type IconSize } from '../primitives/icon';

  interface Props {
    size?: IconSize;
    class?: string | undefined;
  }

  let { size = 'md', class: className }: Props = $props();
</script>

<svg
  class={cn('shrink-0 text-accent', ICON_SIZES[size], className)}
  viewBox={mark.viewBox.join(' ')}
  aria-hidden="true"
  focusable="false"
>
  {#each mark.strokes as stroke, index (index)}
    <polyline
      points={stroke.points.map((point) => point.join(',')).join(' ')}
      fill="none"
      stroke="currentColor"
      stroke-width={stroke.width}
      stroke-linecap="round"
      stroke-linejoin="round"
    />
  {/each}
</svg>
