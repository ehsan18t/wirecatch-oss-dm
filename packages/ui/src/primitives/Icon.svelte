<script lang="ts">
  // Every icon goes through here, so its size comes from the tokens and never from a number at the call site. Icons are decoration: the text or the label of the control beside them says what they mean.
  import { cn } from '../lib/cn';
  import { ICON_SIZES, type IconComponent, type IconSize } from './icon';

  interface Props {
    icon: IconComponent;
    size?: IconSize;
    class?: string | undefined;
  }

  let { icon: Glyph, size = 'md', class: className }: Props = $props();
</script>

<Glyph class={cn('shrink-0', ICON_SIZES[size], className)} aria-hidden="true" focusable="false" />
