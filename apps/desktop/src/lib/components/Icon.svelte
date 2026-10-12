<script lang="ts">
  // Every icon in the app goes through here, so its size and stroke come from the tokens in
  // app.css rather than from a number at each call site. Icons are decoration: the text or the
  // label of the control next to them says what they mean.
  import type { LucideIcon } from '@lucide/svelte';
  import type { IconSize } from './scale';

  let {
    icon: Glyph,
    size,
    thin = false,
  }: {
    icon: LucideIcon;
    size: IconSize;
    /** The lighter stroke used for file type pictures. */
    thin?: boolean;
  } = $props();
</script>

<Glyph class={[`icon-${size}`, thin && 'icon-thin']} aria-hidden="true" />
