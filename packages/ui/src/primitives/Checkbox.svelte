<script lang="ts">
  // A native checkbox drawn in the design's colours. Being native, it keeps the keyboard, the form and the screen reader behaviour; in forced colours it goes back to the system's own drawing.
  import Check from '~icons/lucide/check';
  import Minus from '~icons/lucide/minus';
  import type { Snippet } from 'svelte';
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { cn } from '../lib/cn';
  import { DISABLED, FOCUS_RING } from '../recipes/control';
  import Icon from './Icon.svelte';

  interface Props extends Omit<
    HTMLInputAttributes,
    'type' | 'class' | 'checked' | 'children' | 'onchange' | 'onclick' | 'indeterminate'
  > {
    checked?: boolean;
    /** Some but not all of what it stands for is checked. */
    indeterminate?: boolean;
    /** The visible label; without it, give the box an aria-label. */
    label?: string | undefined;
    /** Shows the label to screen readers only, when the layout already says what the box is. */
    hideLabel?: boolean;
    /** Called with the state a click asks for; the box shows what the owner sets. */
    onchange?: (checked: boolean) => void;
    /** A second line under the label. */
    description?: string | undefined;
    children?: Snippet;
    class?: string | undefined;
  }

  let {
    checked = false,
    indeterminate = false,
    label,
    hideLabel = false,
    onchange,
    description,
    children,
    disabled,
    class: className,
    ...rest
  }: Props = $props();

  const uid = $props.id();
  const labelId = `${uid}-label`;
  const hintId = `${uid}-hint`;
  const marked = $derived(checked || indeterminate);

  /** Controlled: the box goes back to what it was told, and shows the change once the owner makes it. */
  function onclick(event: MouseEvent & { currentTarget: HTMLInputElement }) {
    const next = event.currentTarget.checked;
    event.currentTarget.checked = checked;
    event.currentTarget.indeterminate = indeterminate;
    onchange?.(next);
  }

  /** `indeterminate` is a property only, never an attribute. */
  function showIndeterminate(input: HTMLInputElement) {
    input.indeterminate = indeterminate;
  }
</script>

<label
  class={cn(
    'inline-flex items-start gap-2.5 text-ui text-ink select-none',
    disabled && 'cursor-not-allowed opacity-disabled',
    className,
  )}
>
  <span class="relative mt-0.5 inline-flex shrink-0">
    <input
      type="checkbox"
      {checked}
      {disabled}
      {onclick}
      {@attach showIndeterminate}
      aria-labelledby={label === undefined ? undefined : labelId}
      aria-describedby={description ? hintId : undefined}
      class={cn(
        'peer size-4 appearance-none rounded-xs border border-line-strong bg-field transition',
        'enabled:hover:border-ink-muted checked:border-accent checked:bg-accent indeterminate:border-accent indeterminate:bg-accent',
        'enabled:active:scale-pressed forced-colors:appearance-auto',
        FOCUS_RING,
        DISABLED,
      )}
      {...rest}
    />
    {#if marked}
      <Icon
        icon={indeterminate ? Minus : Check}
        size="xs"
        class="pointer-events-none absolute inset-0 m-auto animate-pop-in text-accent-fg forced-colors:hidden"
      />
    {/if}
  </span>
  {#if label !== undefined || children}
    <span class="flex min-w-0 flex-col">
      {#if label}<span id={labelId} class={cn(hideLabel && 'sr-only')}>{label}</span>{/if}
      {@render children?.()}
      {#if description}
        <span id={hintId} class="text-small text-ink-muted">{description}</span>
      {/if}
    </span>
  {/if}
</label>
