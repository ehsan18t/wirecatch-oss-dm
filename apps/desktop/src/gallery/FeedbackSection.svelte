<script lang="ts" module>
  /** How often the sample bar moves, and by how much, so its motion can be judged. */
  const TICK_MS = 700;
  const STEP = 0.04;
</script>

<script lang="ts">
  import Inbox from '~icons/lucide/inbox';
  import Plus from '~icons/lucide/plus';
  import {
    Badge,
    Button,
    Callout,
    Card,
    EmptyState,
    Kbd,
    ProgressBar,
    Separator,
    Skeleton,
    Spinner,
    TONES,
  } from '@wirecatch/ui';
  import GallerySection from './GallerySection.svelte';
  import { DOWNLOADS, MOVING_START, SAMPLE_COUNT, SKELETON_ROWS } from './sample';
  import Specimen from './Specimen.svelte';

  let moving = $state(MOVING_START);
  const settled = DOWNLOADS.filter((download) => download.state !== 'active');

  $effect(() => {
    const timer = setInterval(() => {
      moving = moving >= 1 ? 0 : Math.min(1, moving + STEP);
    }, TICK_MS);
    return () => {
      clearInterval(timer);
    };
  });
</script>

<GallerySection
  id="feedback"
  title="Feedback"
  description="Progress, status and notices. A bar never announces each percent; it says its state through its value text, and the accent is kept for data that is moving."
>
  <Specimen title="Progress" class="flex-col items-stretch gap-4">
    <div class="flex items-center gap-4">
      <span class="w-16 text-small text-ink-muted">active</span>
      <ProgressBar value={moving} label="Sample active download" class="flex-1" />
    </div>
    {#each settled as download (download.name)}
      <div class="flex items-center gap-4">
        <span class="w-16 text-small text-ink-muted">{download.state}</span>
        <ProgressBar
          value={download.value}
          state={download.state}
          label={download.name}
          class="flex-1"
        />
      </div>
    {/each}
    <div class="flex items-center gap-4">
      <span class="w-16 text-small text-ink-muted">unknown</span>
      <ProgressBar value={null} label="Getting video details" class="flex-1" />
    </div>
    <div class="grid grid-cols-4 items-center gap-4">
      <ProgressBar value={moving} size="xs" label="Extra small" />
      <ProgressBar value={moving} size="sm" label="Small" />
      <ProgressBar value={moving} size="md" label="Medium" />
      <ProgressBar value={moving} size="lg" label="Large" />
    </div>
  </Specimen>

  <div class="grid gap-4 md:grid-cols-2">
    <Specimen title="Badges">
      {#each TONES as tone (tone)}
        <Badge {tone}>{tone}</Badge>
      {/each}
      {#each TONES as tone (tone)}
        <Badge {tone} solid size="sm">{tone === 'neutral' ? SAMPLE_COUNT : tone}</Badge>
      {/each}
      <Badge tone="accent" dot>Downloading</Badge>
      <Badge tone="warning" dot>Waiting for the network</Badge>
    </Specimen>
    <Specimen title="Spinners and keys">
      <Spinner size="sm" />
      <Spinner size="lg" />
      <Spinner size="2xl" />
      <Spinner label="Getting video details">
        <span class="text-ui text-ink-muted">Getting video details…</span>
      </Spinner>
      <Separator orientation="vertical" class="h-6" />
      <Kbd keys={['Ctrl', 'V']} />
      <Kbd keys={['Ctrl', 'Shift', 'P']} />
    </Specimen>
  </div>

  <Specimen title="Callouts" class="flex-col items-stretch">
    <Callout tone="danger" title="The download folder is not available">
      D:\Downloads is on a drive that is not connected.
      {#snippet actions()}<Button size="sm">Choose another</Button>{/snippet}
    </Callout>
    <Callout tone="warning" title="The link expires soon"
      >Refresh it before the download resumes.</Callout
    >
    <Callout tone="success">Every download in the queue finished.</Callout>
    <Callout tone="info">Downloads caught from the browser open a small window first.</Callout>
  </Specimen>

  <div class="grid gap-4 md:grid-cols-2">
    <Card padding="none">
      <EmptyState
        icon={Inbox}
        title="Nothing downloading"
        description="Paste a link or catch one from the browser, and it shows up here."
        heading="h3"
      >
        {#snippet actions()}
          <Button variant="primary" icon={Plus}>New download</Button>
        {/snippet}
      </EmptyState>
    </Card>
    <Card class="flex flex-col gap-4">
      <h3 class="text-ui font-semibold text-ink">Loading</h3>
      {#each SKELETON_ROWS as row (row)}
        <div class="flex items-center gap-3">
          <Skeleton class="size-9 rounded-sm" />
          <div class="flex flex-1 flex-col gap-2">
            <Skeleton class="h-3 w-2/3" />
            <Skeleton class="h-2 w-1/3" />
          </div>
        </div>
      {/each}
      <Card variant="outline" padding="sm" interactive class="text-ui text-ink-muted">
        An outlined card that reacts to the pointer.
      </Card>
    </Card>
  </div>
</GallerySection>
