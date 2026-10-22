<script lang="ts">
  // A download's speed over the last minute, while its details are open: sampled once a second from the row, drawn as a line in the accent over a tint of the area under it, on a baseline, with its peak and average written under it. The line keeps a quarter of the height above its peak, so a steady speed reads as a level line rather than one stuck to the top edge. The drawing is a picture of the figures, so screen readers read only those.
  import type { DownloadSummary } from '@wirecatch/protocol';
  import { formatSpeedShown } from '@wirecatch/units';
  import { untrack } from 'svelte';

  let { row }: { row: Pick<DownloadSummary, 'speed'> } = $props();

  /** Samples kept: a minute at one a second. */
  const SAMPLES = 60;
  const SAMPLE_MS = 1000;
  /** The drawing's own units; it stretches to the width it is given. */
  const WIDTH = 600;
  const HEIGHT = 56;
  /** Room kept below the slowest sample, so the line is never clipped. */
  const BOTTOM = 2;
  /** The scale's top, as a share of the peak. */
  const HEADROOM = 1.25;

  let samples = $state<number[]>([]);

  $effect(() => {
    // The first sample counts twice, so a line shows from the moment the details open.
    const take = () => {
      samples =
        samples.length === 0
          ? [row.speed, row.speed]
          : [...samples.slice(-(SAMPLES - 1)), row.speed];
    };
    untrack(take);
    const timer = setInterval(take, SAMPLE_MS);
    return () => clearInterval(timer);
  });

  /** The line, stretched across the whole width from the first sample to the last, and the area under it. */
  const shape = $derived.by(() => {
    if (samples.length < 2) return null;
    const most = Math.max(...samples, 1) * HEADROOM;
    const step = WIDTH / (samples.length - 1);
    const points = samples.map((v, i) => {
      const x = i * step;
      const y = HEIGHT - BOTTOM - (v / most) * (HEIGHT - BOTTOM);
      return `${x.toFixed(1)} ${y.toFixed(1)}`;
    });
    const line = `M${points.join(' L')}`;
    return { line, area: `${line} L${String(WIDTH)} ${String(HEIGHT)} L0 ${String(HEIGHT)} Z` };
  });

  /** The fastest and the average of the minute, as the caption says them. */
  const figures = $derived.by(() => {
    if (samples.length < 2) return null;
    // The first sample was counted twice to start the line; the figures count it once.
    const taken = samples.slice(1);
    const peak = Math.max(...taken, row.speed);
    const average = taken.reduce((sum, v) => sum + v, 0) / taken.length;
    return `Peak ${formatSpeedShown(peak)} · average ${formatSpeedShown(average)}`;
  });
</script>

{#if shape}
  <figure class="flex flex-col gap-1">
    <svg
      viewBox="0 0 {WIDTH} {HEIGHT}"
      preserveAspectRatio="none"
      class="h-14 w-full overflow-visible border-b border-line text-accent"
      aria-hidden="true"
    >
      <path d={shape.area} class="fill-accent/tint" stroke="none" />
      <path
        d={shape.line}
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        stroke-linejoin="round"
        vector-effect="non-scaling-stroke"
      />
    </svg>
    <figcaption class="flex justify-between text-caption text-ink-subtle tabular-nums">
      <span>Last minute</span>
      <span>{figures}</span>
    </figcaption>
  </figure>
{/if}
