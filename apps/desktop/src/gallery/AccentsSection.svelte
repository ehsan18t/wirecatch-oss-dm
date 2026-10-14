<script lang="ts">
  import { ACCENTS, Badge, Button, ProgressBar, Switch, type Accent } from '@wirecatch/ui';
  import DownloadRowSample from './DownloadRowSample.svelte';
  import GallerySection from './GallerySection.svelte';
  import { DOWNLOADS, PART_WAY, SYSTEM_ACCENT_SAMPLE } from './sample';
  import Specimen from './Specimen.svelte';

  let on = $state(true);

  const title = (accent: Accent) =>
    accent === 'system' ? 'System' : `${accent.charAt(0).toUpperCase()}${accent.slice(1)}`;
</script>

<GallerySection
  id="accents"
  title="Accents and schemes"
  description="Every accent in both schemes. An accent is one tone per scheme (deep on light, bright on dark); hover, pressed, the soft tint and the text on it are derived. Any part of a page can take its own scheme or accent."
>
  <div class="grid grid-cols-2 gap-3 md:grid-cols-4">
    {#each ACCENTS as accent (accent)}
      <div
        data-accent={accent}
        style:--accent-system={accent === 'system' ? SYSTEM_ACCENT_SAMPLE : undefined}
        class="flex flex-col gap-3 rounded-lg border border-line bg-raised p-4 shadow-raised"
      >
        <div class="flex items-center justify-between">
          <span class="text-ui font-semibold text-ink">{title(accent)}</span>
          <Badge tone="accent" size="sm">{accent === 'cyan' ? 'Default' : 'Accent'}</Badge>
        </div>
        <ProgressBar value={PART_WAY} label="{title(accent)} progress" size="sm" />
        <div class="flex items-center justify-between gap-2">
          <Button variant="primary" size="sm">Download</Button>
          <Switch
            checked={on}
            label="{title(accent)} switch"
            hideLabel
            onchange={(next: boolean) => (on = next)}
          />
        </div>
      </div>
    {/each}
  </div>

  <div class="grid gap-4 lg:grid-cols-2">
    {#each ['light', 'dark'] as const as scheme (scheme)}
      <div data-scheme={scheme} class="rounded-xl border border-line bg-base p-2 text-ink">
        <div class="rounded-lg border border-line bg-layer p-2">
          <p
            class="px-3 pt-1 pb-2 text-caption font-semibold tracking-caps text-ink-subtle uppercase"
          >
            {scheme} scheme
          </p>
          {#each DOWNLOADS as row (row.name)}
            <DownloadRowSample {...row} />
          {/each}
        </div>
      </div>
    {/each}
  </div>

  <Specimen title="Download rows" note="Hover a row for its actions">
    <div class="w-full">
      {#each DOWNLOADS as row (row.name)}
        <DownloadRowSample {...row} />
      {/each}
    </div>
  </Specimen>
</GallerySection>
