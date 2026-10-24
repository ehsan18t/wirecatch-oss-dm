<script lang="ts">
  // "Media on this page": the video and audio the extension saw on the active tab, each ready to send to Wirecatch. A page with none shows no section at all (D70).
  import { Badge, Card } from '@wirecatch/ui';
  import type { MediaView } from '../../lib/media/center';
  import type { MediaStatus } from '../../lib/popup/model.svelte';
  import MediaItem from './MediaItem.svelte';

  interface Props {
    /** The active tab's list; null while it is on its way. */
    view: MediaView | null;
    statuses: Record<string, MediaStatus>;
    ondownload: (key: string) => void;
  }

  let { view, statuses, ondownload }: Props = $props();

  const uid = $props.id();
  const titleId = `${uid}-title`;
  const entries = $derived(view?.page === 'web' ? view.entries : []);
</script>

{#if entries.length > 0}
  <Card as="section" padding="none" aria-labelledby={titleId}>
    <div class="flex items-center gap-2 px-4 py-3">
      <h2 id={titleId} class="flex-1 text-ui font-semibold text-ink">Media on this page</h2>
      <Badge size="sm" tone="accent">{entries.length}</Badge>
    </div>
    <ul class="divide-y divide-line border-t border-line">
      {#each entries as entry (entry.key)}
        <MediaItem
          {entry}
          status={statuses[entry.key]}
          ondownload={() => {
            ondownload(entry.key);
          }}
        />
      {/each}
    </ul>
  </Card>
{/if}
