<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import { TONE_SOFT, type Tone } from '../recipes/control';
  import Icon from './Icon.svelte';
  import type { IconComponent } from './icon';

  interface Props {
    title: string;
    description?: string | undefined;
    icon?: IconComponent | undefined;
    tone?: Tone;
    /** The heading the title takes in the page's outline. */
    heading?: 'h2' | 'h3' | 'h4';
    /** What the person can do next: one or two buttons. */
    actions?: Snippet;
    class?: string | undefined;
  }

  let {
    title,
    description,
    icon,
    tone = 'accent',
    heading = 'h2',
    actions,
    class: className,
  }: Props = $props();
</script>

<div
  class={cn(
    'flex animate-rise-in flex-col items-center justify-center gap-3 px-6 py-10 text-center',
    className,
  )}
>
  {#if icon}
    <div
      class={cn('mb-1 flex size-14 items-center justify-center rounded-xl', TONE_SOFT[tone])}
      aria-hidden="true"
    >
      <Icon {icon} size="3xl" />
    </div>
  {/if}
  <svelte:element this={heading} class="font-display text-subtitle font-semibold text-ink">
    {title}
  </svelte:element>
  {#if description}
    <p class="max-w-prose text-body text-ink-muted">{description}</p>
  {/if}
  {#if actions}
    <div class="mt-2 flex flex-wrap items-center justify-center gap-2">{@render actions()}</div>
  {/if}
</div>
