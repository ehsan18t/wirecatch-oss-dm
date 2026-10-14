<script lang="ts">
  // A download's kind of file, as a picture on a soft tile: the list's first column and the details' header.
  import type { Category } from '@wirecatch/protocol';
  import { cn, Icon, type IconSize } from '@wirecatch/ui';
  import { CATEGORY_ICON } from './category-icons';

  interface Props {
    category: Category;
    /** The tile's size: a list row's, or the details' larger one. */
    size?: 'md' | 'lg';
    class?: string | undefined;
  }

  let { category, size = 'md', class: className }: Props = $props();

  const GLYPH: Record<'md' | 'lg', IconSize> = { md: 'lg', lg: '2xl' };
</script>

<span
  class={cn(
    'flex shrink-0 items-center justify-center rounded-md bg-wash-strong text-ink-muted',
    size === 'md' ? 'size-9' : 'size-12',
    className,
  )}
  aria-hidden="true"
>
  <Icon icon={CATEGORY_ICON[category]} size={GLYPH[size]} />
</span>
