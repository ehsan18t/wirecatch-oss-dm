<script lang="ts">
  // How fast everything moves right now, in the title bar, and in its tooltip what is going on. It is not a live region: a speed changes every second, and a screen reader would hear nothing else.
  import ArrowDown from '~icons/lucide/arrow-down';
  import { formatSpeed } from '@wirecatch/units';
  import { cn, Icon, tooltip } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';

  const app = getApp();

  /** What the readout says when no bytes move, by what is going on. */
  const HEADLINE = { idle: 'Idle', finishing: 'Finishing videos', starting: 'Starting' } as const;

  const speed = $derived(formatSpeed(app.downloads.totalSpeed));
  const active = $derived(app.counts.status.active);
  const waiting = $derived(app.counts.status.waiting);
  const joining = $derived(app.counts.joining);
  const headline = $derived<keyof typeof HEADLINE>(
    active === 0 ? 'idle' : joining === active ? 'finishing' : 'starting',
  );
  const summary = $derived.by(() => {
    if (active + waiting === 0) return 'Nothing running';
    const downloading = active - joining;
    return [
      downloading > 0 || joining === 0 ? `${String(downloading)} downloading` : '',
      joining > 0 ? `${String(joining)} being joined or converted` : '',
      waiting > 0 ? `${String(waiting)} waiting` : '',
    ]
      .filter(Boolean)
      .join(', ');
  });
</script>

<p
  class={cn(
    'flex h-control items-center gap-1.5 rounded-sm px-2 text-small tabular-nums',
    speed ? 'text-ink' : 'text-ink-subtle',
  )}
  {@attach tooltip(() => `Total download speed. ${summary}.`)}
>
  <Icon icon={ArrowDown} size="sm" class={speed ? 'text-accent' : undefined} />
  <span class="sr-only">Total download speed:</span>
  <span>{speed || HEADLINE[headline]}</span>
  <span class="sr-only">. {summary}.</span>
</p>
