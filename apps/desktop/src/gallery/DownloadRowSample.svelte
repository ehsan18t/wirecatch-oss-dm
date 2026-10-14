<script lang="ts">
  // What a download row could look like on these pieces: a file, its progress wire, and actions that show on hover. A sample for judging the look, not the list later units build.
  import FileArchive from '~icons/lucide/file-archive';
  import FolderOpen from '~icons/lucide/folder-open';
  import Pause from '~icons/lucide/pause';
  import Play from '~icons/lucide/play';
  import RotateCcw from '~icons/lucide/rotate-ccw';
  import {
    Badge,
    Icon,
    IconButton,
    ProgressBar,
    type ProgressState,
    type Tone,
  } from '@wirecatch/ui';

  interface Props {
    name: string;
    detail: string;
    value: number | null;
    state: ProgressState;
  }

  let { name, detail, value, state }: Props = $props();

  const STATUS: Record<ProgressState, { label: string; tone: Tone }> = {
    active: { label: 'Downloading', tone: 'accent' },
    paused: { label: 'Paused', tone: 'warning' },
    error: { label: 'Failed', tone: 'danger' },
    done: { label: 'Finished', tone: 'success' },
    queued: { label: 'Waiting', tone: 'neutral' },
  };
</script>

<div class="group relative flex items-center gap-3 rounded-md px-3 py-2.5 transition hover:bg-wash">
  <div
    class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-wash-strong text-ink-muted"
  >
    <Icon icon={FileArchive} size="lg" />
  </div>
  <div class="flex min-w-0 flex-1 flex-col gap-1.5">
    <div class="flex min-w-0 items-center gap-2">
      <span class="truncate text-ui font-medium text-ink">{name}</span>
      <Badge tone={STATUS[state].tone} size="sm" dot>{STATUS[state].label}</Badge>
    </div>
    <ProgressBar {value} {state} size="xs" label={name} />
    <span class="text-small text-ink-muted tabular-nums">{detail}</span>
  </div>
  <div
    class="absolute top-1/2 right-2 flex -translate-y-1/2 items-center gap-0.5 rounded-sm border border-line bg-overlay p-0.5 opacity-0 shadow-raised glass transition group-focus-within:opacity-100 group-hover:opacity-100"
  >
    {#if state === 'active'}
      <IconButton icon={Pause} label="Pause" size="sm" />
    {:else if state === 'paused'}
      <IconButton icon={Play} label="Resume" size="sm" />
    {:else if state === 'error'}
      <IconButton icon={RotateCcw} label="Try again" size="sm" />
    {/if}
    <IconButton icon={FolderOpen} label="Show in folder" size="sm" />
  </div>
</div>
