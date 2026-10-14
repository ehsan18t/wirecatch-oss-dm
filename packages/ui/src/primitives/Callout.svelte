<script lang="ts" module>
  import CircleAlert from '~icons/lucide/circle-alert';
  import CircleCheck from '~icons/lucide/circle-check';
  import Info from '~icons/lucide/info';
  import TriangleAlert from '~icons/lucide/triangle-alert';
  import type { Tone } from '../recipes/control';
  import type { IconComponent } from './icon';

  const TONE_ICON: Record<Tone, IconComponent> = {
    neutral: Info,
    accent: Info,
    info: Info,
    success: CircleCheck,
    warning: TriangleAlert,
    danger: CircleAlert,
  };
</script>

<script lang="ts">
  // A notice inside the page: a problem with the download folder, a hint, a result. It is not a live region by default; pass `live` when it appears in answer to something the person did.
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import { TONE_TEXT, TONE_TINT } from '../recipes/control';
  import Icon from './Icon.svelte';

  interface Props {
    tone?: Tone;
    title?: string | undefined;
    /** Another icon than the tone's own. */
    icon?: IconComponent | undefined;
    /** Announce it when it appears: `polite` for news, `assertive` for a failure the person must know of now. */
    live?: 'polite' | 'assertive' | undefined;
    /** Buttons at the end, such as "Choose another folder". */
    actions?: Snippet;
    class?: string | undefined;
    children?: Snippet;
  }

  let { tone = 'info', title, icon, live, actions, class: className, children }: Props = $props();
</script>

<div
  role={live === 'assertive' ? 'alert' : live === 'polite' ? 'status' : undefined}
  class={cn(
    'flex items-start gap-3 rounded-md border px-3.5 py-3 text-ui text-ink forced-colors:border-sys-text',
    TONE_TINT[tone],
    className,
  )}
>
  <Icon icon={icon ?? TONE_ICON[tone]} size="lg" class={cn('mt-px', TONE_TEXT[tone])} />
  <div class="flex min-w-0 flex-1 flex-col gap-0.5">
    {#if title}<p class="font-semibold">{title}</p>{/if}
    {#if children}<div class="text-ink-muted">{@render children()}</div>{/if}
  </div>
  {#if actions}
    <div class="flex shrink-0 items-center gap-2 self-center">{@render actions()}</div>
  {/if}
</div>
