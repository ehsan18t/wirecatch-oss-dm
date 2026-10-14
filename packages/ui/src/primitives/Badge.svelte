<script lang="ts">
  import type { Snippet } from 'svelte';
  import { tv } from '../lib/cn';
  import { TONE_FILL, TONE_SOFT, TONE_SOLID, type Tone } from '../recipes/control';
  import Icon from './Icon.svelte';
  import type { IconComponent } from './icon';

  const badge = tv({
    base: 'inline-flex shrink-0 items-center gap-1 rounded-full font-semibold whitespace-nowrap tabular-nums forced-colors:border forced-colors:border-sys-text',
    variants: {
      size: {
        sm: 'h-4.5 px-1.5 text-badge',
        md: 'h-5.5 px-2 text-caption',
      },
      solid: { true: '', false: '' },
    },
    defaultVariants: { size: 'md', solid: false },
  });

  interface Props {
    tone?: Tone;
    /** A filled pill, for counts that call for attention. */
    solid?: boolean;
    size?: 'sm' | 'md';
    /** A small dot in the tone before the text: a status at a glance. */
    dot?: boolean;
    icon?: IconComponent | undefined;
    class?: string | undefined;
    children: Snippet;
  }

  let {
    tone = 'neutral',
    solid = false,
    size,
    dot = false,
    icon,
    class: className,
    children,
  }: Props = $props();
</script>

<span
  class={badge({ size, solid, class: [solid ? TONE_SOLID[tone] : TONE_SOFT[tone], className] })}
>
  {#if dot}
    <span class="size-1.5 rounded-full {solid ? 'bg-current' : TONE_FILL[tone]}" aria-hidden="true"
    ></span>
  {/if}
  {#if icon}<Icon {icon} size="xs" />{/if}
  {@render children()}
</span>
