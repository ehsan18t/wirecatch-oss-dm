<script lang="ts">
  // A native range input: arrow, Page and Home/End keys, and what screen readers expect, come with it. The filled part of the track is drawn from the value; the thumb is Windows 11's, an accent dot in a ring that grows under the pointer and shrinks while held.
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { cn } from '../lib/cn';
  import { fraction, wholePercent } from '../lib/percent';
  import { DISABLED, FOCUS_RING } from '../recipes/control';

  interface Props extends Omit<
    HTMLInputAttributes,
    'type' | 'class' | 'value' | 'min' | 'max' | 'step' | 'onchange' | 'oninput' | 'aria-label'
  > {
    value: number;
    /** The name when no Field labels it. */
    label?: string | undefined;
    min?: number;
    max?: number;
    step?: number;
    /** What the value means, read by screen readers instead of the bare number ("4 connections"). */
    valueText?: ((value: number) => string) | undefined;
    class?: string | undefined;
    /** Called as the thumb moves. */
    oninput?: (value: number) => void;
    /** Called when the person lets go: the value to keep. */
    onchange?: (value: number) => void;
  }

  let {
    value,
    min = 0,
    max = 100,
    step = 1,
    valueText,
    label,
    class: className,
    oninput,
    onchange,
    ...rest
  }: Props = $props();

  const fill = $derived(wholePercent(fraction(value - min, max - min)));
</script>

<input
  type="range"
  {min}
  {max}
  {step}
  {value}
  aria-label={label}
  aria-valuetext={valueText?.(value)}
  style:--slider-fill="{fill}%"
  class={cn(
    'slider-track h-1 w-full cursor-default appearance-none rounded-full transition',
    'slider-thumb:size-5 slider-thumb:appearance-none slider-thumb:rounded-full slider-thumb:border-4 slider-thumb:border-thumb slider-thumb:bg-accent slider-thumb:shadow-thumb',
    'slider-thumb:transition slider-thumb:duration-normal slider-thumb:ease-spring hover:slider-thumb:border-3 active:slider-thumb:border-5',
    'forced-colors:appearance-auto',
    FOCUS_RING,
    DISABLED,
    className,
  )}
  oninput={(event) => oninput?.(event.currentTarget.valueAsNumber)}
  onchange={(event) => onchange?.(event.currentTarget.valueAsNumber)}
  {...rest}
/>
