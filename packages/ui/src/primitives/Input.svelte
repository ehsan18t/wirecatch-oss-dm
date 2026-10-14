<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { cn } from '../lib/cn';
  import { TextSync } from '../lib/controlled-text';
  import { FIELD_INPUT, field, type FieldSize } from '../recipes/field';
  import Icon from './Icon.svelte';
  import type { IconComponent } from './icon';

  interface Props extends Omit<
    HTMLInputAttributes,
    'class' | 'size' | 'value' | 'oninput' | 'onchange' | 'aria-label'
  > {
    value?: string;
    /** The name when no Field labels it. */
    label?: string | undefined;
    /** Called with the text as it is typed. The field shows the value the owner sets: a rewrite at once, a refusal once the field is left; an owner may answer later (TextSync in lib/controlled-text.ts). */
    onchange?: (value: string) => void;
    size?: FieldSize;
    /** Marks the value wrong; pair with a message (Field's `error`). */
    invalid?: boolean;
    /** An icon at the start of the box, such as a search glass. */
    icon?: IconComponent | undefined;
    /** Something at the end of the box: a unit, a clear button. */
    trailing?: Snippet;
    /** The input element, for focusing it from outside. */
    ref?: HTMLInputElement | null;
    class?: string | undefined;
  }

  let {
    value = '',
    label,
    onchange,
    size,
    invalid = false,
    icon,
    trailing,
    ref = $bindable(null),
    type = 'text',
    class: className,
    ...rest
  }: Props = $props();

  const sync = new TextSync();
  $effect(() => {
    sync.show(value);
  });
</script>

<div class={field({ size, invalid, class: className })}>
  {#if icon}
    <Icon {icon} size="sm" class="text-ink-subtle" />
  {/if}
  <input
    bind:this={ref}
    {type}
    class={cn(FIELD_INPUT)}
    aria-label={label}
    aria-invalid={invalid || undefined}
    {@attach sync.attach}
    oninput={(event) => {
      sync.typed(event.currentTarget.value);
      onchange?.(event.currentTarget.value);
    }}
    {...rest}
  />
  {@render trailing?.()}
</div>
