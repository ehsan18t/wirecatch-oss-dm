<script lang="ts" module>
  import type { DialogSize as Width } from '@wirecatch/ui';
  import type { DialogSize } from './scale';

  const WIDTH: Record<DialogSize, Width> = { medium: 'md', wide: 'lg' };
</script>

<script lang="ts">
  // A dialog for the screens styled by app.css: the design system's dialog (a solid panel over the one blur, its spacing, motion, focus and Escape), sized by name. What goes inside keeps the styles of the screen it belongs to.
  import type { Snippet } from 'svelte';
  import { Dialog } from '@wirecatch/ui';

  let {
    title,
    description,
    onclose,
    children,
    footer,
    size = 'medium',
  }: {
    title: string;
    description?: string;
    onclose: () => void;
    children: Snippet;
    footer?: Snippet;
    size?: DialogSize;
  } = $props();
</script>

<Dialog open {title} {description} size={WIDTH[size]} {onclose} {footer}>
  {@render children()}
</Dialog>
