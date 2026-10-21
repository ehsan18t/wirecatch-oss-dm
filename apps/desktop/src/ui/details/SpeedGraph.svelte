<script lang="ts">
  // A download's speed over the last minute, while its details are open: sampled once a second from the row, drawn as one line in the accent. It is a picture of the figure written above it, so screen readers skip it.
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

  const path = $derived.by(() => {
    if (samples.length < 2) return '';
    const most = Math.max(...samples, 1);
    const step = WIDTH / (SAMPLES - 1);
    const from = SAMPLES - samples.length;
    return samples
      .map((v, i) => {
        const x = (from + i) * step;
        const y = HEIGHT - BOTTOM - (v / most) * (HEIGHT - TOP - BOTTOM);
        return `${i === 0 ? 'M' : 'L'}${x.toFixed(1)} ${y.toFixed(1)}`;
      })
      .join(' ');
  });
</script>

<svg
  viewBox="0 0 {WIDTH} {HEIGHT}"
  preserveAspectRatio="none"
  class="h-14 w-full overflow-visible text-accent"
  aria-hidden="true"
>
  <path
    d={path}
    fill="none"
    stroke="currentColor"
    stroke-width="1.5"
    stroke-linejoin="round"
    vector-effect="non-scaling-stroke"
  />
</svg>
