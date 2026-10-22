<script lang="ts">
  import type { HTMLTextareaAttributes } from 'svelte/elements';
  import { cn } from '../lib/cn';
  import { TextSync } from '../lib/controlled-text';
  import { FIELD_INPUT, field } from '../recipes/field';

  interface Props extends Omit<
    HTMLTextareaAttributes,
    'class' | 'value' | 'oninput' | 'onchange' | 'aria-label'
  > {
    value?: string;
    /** The name when no Field labels it. */
    label?: string | undefined;
    /** Called with the text as it is typed. The field shows the value the owner sets: a rewrite at once, a refusal once the field is left; an owner may answer later (TextSync in lib/controlled-text.ts). */
    onchange?: (value: string) => void;
    invalid?: boolean;
    /** Monospaced text, for lists of links and other machine text. */
    mono?: boolean;
    class?: string | undefined;
  }

  let {
    value = '',
    label,
    onchange,
    invalid = false,
    mono = false,
    rows = 4,
    class: className,
    ...rest
  }: Props = $props();

  const sync = new TextSync();
  $effect(() => {
    sync.show(value);
  });
</script>

<div class={field({ invalid, multiline: true, class: className })}>
  <textarea
    {rows}
    aria-label={label}
    {@attach sync.attach}
    oninput={(event) => {
      sync.typed(event.currentTarget.value);
      onchange?.(event.currentTarget.value);
    }}
    class={cn(FIELD_INPUT, 'resize-none py-0.5 leading-normal', mono && 'font-mono text-small')}
    aria-invalid={invalid || undefined}
    {...rest}></textarea>
</div>
