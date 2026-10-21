<script lang="ts">
  // A download's speed over the last minute, while its details are open: sampled once a second from the row, drawn as a line in the accent over a tint of the area under it. It is a picture of the figure written above it, so screen readers skip it.
  import type { DownloadSummary } from '@wirecatch/protocol';
  import { untrack } from 'svelte';

  let { row }: { row: Pick<DownloadSummary, 'speed'> } = $props();

  /** Samples kept: a minute at one a second. */
  const SAMPLES = 60;
  const SAMPLE_MS = 1000;
  /** The drawing's own units; it stretches to the width it is given. */
  const WIDTH = 600;
  const HEIGHT = 56;
  /** Room kept above the fastest sample and below the slowest, so the line is never clipped. */
  const TOP = 4;
  const BOTTOM = 2;

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
    const most = Math.max(...samples, 1);
    const step = WIDTH / (samples.length - 1);
    const points = samples.map((v, i) => {
      const x = i * step;
      const y = HEIGHT - BOTTOM - (v / most) * (HEIGHT - TOP - BOTTOM);
      return `${x.toFixed(1)} ${y.toFixed(1)}`;
    });
    const line = `M${points.join(' L')}`;
    return { line, area: `${line} L${String(WIDTH)} ${String(HEIGHT)} L0 ${String(HEIGHT)} Z` };
  });
</script>

{#if shape}
  <svg
    viewBox="0 0 {WIDTH} {HEIGHT}"
    preserveAspectRatio="none"
    class="h-14 w-full overflow-visible text-accent"
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
{/if}
