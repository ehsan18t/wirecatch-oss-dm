<script lang="ts" module>
  import type { IconSize } from './icon';
  import type { IconButtonSize } from '../recipes/button';

  const GLYPH: Record<IconButtonSize, IconSize> = { xs: 'xs', sm: 'sm', md: 'md', lg: 'lg' };
</script>

<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import { tooltip as withTooltip } from '../attachments/tooltip.svelte';
  import DisabledReason from '../internal/DisabledReason.svelte';
  import { blockedReason, reasonAttributes } from '../internal/disabled-reason';
  import { iconButton, type IconButtonVariants } from '../recipes/button';
  import Icon from './Icon.svelte';
  import type { IconComponent } from './icon';
  import Spinner from './Spinner.svelte';

  interface Props extends Omit<
    HTMLButtonAttributes,
    'class' | 'children' | 'aria-label' | 'aria-labelledby'
  > {
    icon: IconComponent;
    /** The button's name. It is also its tooltip, unless `tooltip` says something else (or `null` turns it off). */
    label: string;
    /** Shows on hover and focus; while the button is disabled it is the reason, and the button stays focusable to say it, as Button does. */
    tooltip?: string | null | undefined;
    size?: IconButtonSize;
    variant?: IconButtonVariants['variant'];
    /** On: an open panel, a pressed toggle; pair with aria-pressed or aria-expanded. */
    active?: boolean;
    /** Busy with its own action: it keeps focus (aria-disabled) and ignores clicks until done. */
    loading?: boolean;
    class?: string | undefined;
  }

  let {
    icon,
    label,
    tooltip,
    size = 'md',
    variant,
    active = false,
    loading = false,
    disabled,
    type = 'button',
    class: className,
    onclick,
    'aria-describedby': describedBy,
    ...rest
  }: Props = $props();

  const reasonId = $props.id();
  const reason = $derived(blockedReason(disabled, loading, tooltip));
  const tip = $derived(tooltip === undefined ? label : tooltip);

  function handleClick(event: MouseEvent & { currentTarget: EventTarget & HTMLButtonElement }) {
    if (reason !== null || loading) event.preventDefault();
    else onclick?.(event);
  }
</script>

<button
  {type}
  class={iconButton({ size, variant, active, class: className })}
  disabled={(disabled === true && reason === null) || undefined}
  {...reasonAttributes({ reason, reasonId, busy: loading, describedBy })}
  aria-busy={loading || undefined}
  aria-label={label}
  {@attach withTooltip(() => tip)}
  {...rest}
  onclick={handleClick}
>
  {#if loading}
    <Spinner size={GLYPH[size]} tone="current" />
  {:else}
    <Icon {icon} size={GLYPH[size]} />
  {/if}
</button>
<DisabledReason id={reasonId} {reason} />
