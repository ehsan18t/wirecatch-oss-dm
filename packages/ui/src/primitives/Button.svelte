<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import { tooltip as withTooltip } from '../attachments/tooltip.svelte';
  import DisabledReason from '../internal/DisabledReason.svelte';
  import { blockedReason, reasonAttributes } from '../internal/disabled-reason';
  import { button, type ButtonSize, type ButtonVariant } from '../recipes/button';
  import Icon from './Icon.svelte';
  import type { IconComponent } from './icon';
  import Spinner from './Spinner.svelte';

  interface Props extends Omit<HTMLButtonAttributes, 'class' | 'children'> {
    variant?: ButtonVariant;
    size?: ButtonSize;
    /** Fills the width of its container. */
    block?: boolean;
    /** Leading icon; a spinner takes its place while `loading`. */
    icon?: IconComponent | undefined;
    /** Trailing icon, such as a chevron on a button that opens a menu. */
    trailingIcon?: IconComponent | undefined;
    loading?: boolean;
    /** Shows on hover and focus; while the button is disabled it is the reason, and the button stays focusable to say it. */
    tooltip?: string | null | undefined;
    class?: string | undefined;
    children: Snippet;
  }

  let {
    variant,
    size,
    block = false,
    icon,
    trailingIcon,
    loading = false,
    tooltip,
    disabled,
    type = 'button',
    class: className,
    children,
    onclick,
    'aria-describedby': describedBy,
    ...rest
  }: Props = $props();

  const reasonId = $props.id();
  const reason = $derived(blockedReason(disabled, loading, tooltip));

  function handleClick(event: MouseEvent & { currentTarget: EventTarget & HTMLButtonElement }) {
    if (reason !== null || loading) event.preventDefault();
    else onclick?.(event);
  }
</script>

<button
  {type}
  class={button({ variant, size, block, class: className })}
  disabled={(disabled === true && reason === null) || undefined}
  {...reasonAttributes({ reason, reasonId, busy: loading, describedBy })}
  aria-busy={loading || undefined}
  {@attach withTooltip(() => tooltip)}
  {...rest}
  onclick={handleClick}
>
  {#if loading}
    <Spinner size="sm" tone="current" />
  {:else if icon}
    <Icon {icon} size="sm" />
  {/if}
  {@render children()}
  {#if trailingIcon}
    <Icon icon={trailingIcon} size="sm" class="-mr-1 opacity-quiet" />
  {/if}
</button>
<!-- Beside the button, not in it: inside, it would become part of the button's name. -->
<DisabledReason id={reasonId} {reason} />
