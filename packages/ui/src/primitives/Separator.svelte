<script lang="ts">
  import { cn } from '../lib/cn';

  interface Props {
    orientation?: 'horizontal' | 'vertical';
    /** A separator that only draws a line is hidden from screen readers; one that divides groups of controls is announced. */
    decorative?: boolean;
    class?: string | undefined;
  }

  let { orientation = 'horizontal', decorative = true, class: className }: Props = $props();
</script>

<div
  role={decorative ? 'none' : 'separator'}
  aria-orientation={decorative ? undefined : orientation}
  class={cn(
    'shrink-0 bg-line forced-colors:bg-sys-text',
    orientation === 'horizontal' ? 'h-px w-full' : 'w-px self-stretch',
    className,
  )}
></div>
