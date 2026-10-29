<script lang="ts">
  // An on/off switch (role switch) on a native button, whose own click answers Enter and Space; acting on keydown as well would toggle twice wherever the browser still clicks after it (Firefox, for the extension).
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import SwitchTrack from '../internal/SwitchTrack.svelte';
  import InfoTip from './InfoTip.svelte';

  interface Props {
    checked: boolean;
    /** The switch's name: shown beside it, or only to screen readers with `hideLabel`. */
    label: string;
    hideLabel?: boolean;
    /** What the switch does, behind an info icon after the label; never changes with its value (D84). */
    help?: string | undefined;
    /** The id of help shown elsewhere, in place of `help`. */
    describedby?: string | undefined;
    /** Waiting for the change to land: the thumb shows a spinner and the switch takes no input. */
    loading?: boolean;
    disabled?: boolean;
    /** The label sits after the switch (`end`), or, as in a settings row (`start`), on a line of the row's width with the switch at its end, the label in medium weight. */
    labelSide?: 'start' | 'end';
    id?: string | undefined;
    class?: string | undefined;
    children?: Snippet;
    onchange?: (checked: boolean) => void;
  }

  let {
    checked,
    label,
    hideLabel = false,
    help,
    describedby,
    loading = false,
    disabled = false,
    labelSide = 'end',
    id,
    class: className,
    children,
    onchange,
  }: Props = $props();

  const uid = $props.id();
  const labelId = `${uid}-label`;
  const hintId = `${uid}-hint`;

  function toggle() {
    if (!disabled && !loading) onchange?.(!checked);
  }
</script>

<!-- Focus follows what is read: in a settings row (`start`) label, info icon, switch; otherwise the switch first. -->
<div
  class={cn(
    labelSide === 'start' ? 'flex w-full items-center gap-x-3' : 'inline-flex items-center gap-3',
    className,
  )}
>
  {#if labelSide === 'end'}
    <SwitchTrack
      {id}
      {checked}
      {loading}
      {disabled}
      {labelId}
      describedby={describedby ?? (help ? hintId : undefined)}
      onclick={toggle}
    />
  {/if}
  <span
    class={cn(
      'flex min-w-0 items-center gap-1',
      labelSide === 'start' && 'flex-1',
      hideLabel && 'sr-only',
    )}
  >
    <span
      id={labelId}
      class={cn(
        'text-ui wrap-break-word text-ink',
        labelSide === 'start' && 'min-w-0 font-medium',
        labelSide === 'start' && disabled && 'text-ink-muted',
      )}>{label}</span
    >
    {#if help && !hideLabel}<InfoTip {label} text={help} />{/if}
  </span>
  {@render children?.()}
  {#if help}<span id={hintId} class="sr-only">{help}</span>{/if}
  {#if labelSide === 'start'}
    <SwitchTrack
      {id}
      {checked}
      {loading}
      {disabled}
      {labelId}
      describedby={describedby ?? (help ? hintId : undefined)}
      onclick={toggle}
    />
  {/if}
</div>
