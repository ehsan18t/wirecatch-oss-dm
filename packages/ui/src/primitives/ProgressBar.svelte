<script lang="ts" module>
  import { tv } from '../lib/cn';
  import type { ProgressState } from './progress';

  const bar = tv({
    slots: {
      track:
        'relative w-full overflow-hidden rounded-full bg-track forced-colors:border forced-colors:border-sys-text',
      fill: 'absolute inset-y-0 left-0 w-full origin-left rounded-full transition-transform duration-slower ease-out forced-colors:bg-sys-highlight',
    },
    variants: {
      size: {
        /** The wire under a list row. */
        xs: { track: 'h-0.5' },
        sm: { track: 'h-1' },
        md: { track: 'h-1.5' },
        lg: { track: 'h-2.5' },
      },
      state: {
        active: { fill: 'bg-progress' },
        paused: { fill: 'bg-progress-paused' },
        error: { fill: 'bg-progress-error' },
        done: { fill: 'bg-progress-done' },
        queued: { fill: 'bg-progress-queued' },
      },
    },
    defaultVariants: { size: 'md', state: 'active' },
  });
</script>

<script lang="ts">
  // A linear progress bar (role progressbar). With no value it runs indeterminate; while data moves a soft sheen runs along the fill, which stops for reduced motion. It names its state for screen readers through `valueText` and is never a live region: announcing every percent would drown everything else out.
  import { cn } from '../lib/cn';
  import { fraction, PERCENT, wholePercent } from '../lib/percent';

  interface Props {
    /** Null runs the bar indeterminate: the work has started but its size is unknown. */
    value: number | null;
    max?: number;
    state?: ProgressState;
    size?: 'xs' | 'sm' | 'md' | 'lg';
    /** The bar's name ("Downloading ubuntu.iso"). */
    label: string;
    /** What screen readers read for the value ("45%, paused"); the percentage when not given. */
    valueText?: string | undefined;
    class?: string | undefined;
  }

  let {
    value,
    max = 1,
    state = 'active',
    size,
    label,
    valueText,
    class: className,
  }: Props = $props();

  const share = $derived(value === null ? null : fraction(value, max));
  const percent = $derived(share === null ? undefined : wholePercent(share));
  const styles = $derived(bar({ size, state }));
  const flowing = $derived(state === 'active' && share !== null && share > 0 && share < 1);
</script>

<div
  role="progressbar"
  aria-label={label}
  aria-valuemin={0}
  aria-valuemax={PERCENT}
  aria-valuenow={percent}
  aria-valuetext={valueText ?? (percent === undefined ? undefined : `${String(percent)}%`)}
  aria-busy={share === null || undefined}
  class={styles.track({ class: className })}
>
  {#if share === null}
    <div
      class={cn(styles.fill(), 'w-1/3 animate-activity transition-none')}
      data-state={state}
    ></div>
  {:else}
    <div class={styles.fill()} style:scale="{share} 1" data-state={state}>
      {#if flowing}
        <div
          class="absolute inset-0 animate-flow bg-linear-to-r from-transparent via-white/sheen to-transparent forced-colors:hidden"
          aria-hidden="true"
        ></div>
      {/if}
    </div>
  {/if}
</div>
