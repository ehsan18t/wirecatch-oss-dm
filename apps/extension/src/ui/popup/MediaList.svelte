<script lang="ts">
  // "Media on this page": the video and audio the extension saw on the active tab, each ready to send to Wirecatch, and the other ways to send it.
  import { Badge, Button, Card, Kbd } from '@wirecatch/ui';
  import type { MediaView } from '../../lib/media/center';
  import type { MediaStatus } from '../../lib/popup/model.svelte';
  import MediaItem from './MediaItem.svelte';

  interface Props {
    /** The active tab's list; null while it is on its way. */
    view: MediaView | null;
    statuses: Record<string, MediaStatus>;
    /** The "Show download button on videos" setting; null before settings are known. */
    buttonOn: boolean | null;
    /** The keyboard shortcut that downloads a page's media; null when none is set. */
    shortcut: string | null;
    ondownload: (key: string) => void;
    onsettings: () => void;
  }

  let { view, statuses, buttonOn, shortcut, ondownload, onsettings }: Props = $props();

  const uid = $props.id();
  const titleId = `${uid}-title`;
  const entries = $derived(view?.page === 'web' ? view.entries : []);
  const keys = $derived(shortcut === null ? [] : shortcut.split('+'));
  const message = $derived.by(() => {
    if (view === null) return 'Looking for video and audio on this page.';
    if (view.page === 'private') return 'Wirecatch does not take media from private windows.';
    if (view.page === 'other') return 'Open a web page with video or audio to see it here.';
    return 'No video or audio found on this page yet. Start playing it, and it shows up here.';
  });
</script>

<section aria-labelledby={titleId} class="flex flex-col gap-2">
  <div class="flex items-center gap-2 px-1">
    <h2 id={titleId} class="text-ui font-semibold text-ink">Media on this page</h2>
    {#if entries.length > 0}
      <Badge size="sm" tone="accent">{entries.length}</Badge>
    {/if}
  </div>
  <Card padding="none">
    {#if entries.length > 0}
      <ul class="divide-y divide-line">
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
    {:else}
      <p class="px-4 py-3 text-ui text-ink-muted">{message}</p>
    {/if}
  </Card>
  <div class="flex flex-col gap-1 px-1 text-small text-ink-muted">
    {#if keys.length > 0}
      <p>
        On a page, press <Kbd {keys} /> to send its video or audio to Wirecatch{entries.length > 0
          ? ', or use the Download buttons in this list.'
          : '.'}
      </p>
    {:else}
      <p>
        To send a page's video or audio with a key press, set a shortcut in your browser's extension
        shortcuts.
      </p>
    {/if}
    <!-- The button over videos is taught while the list is empty; with items here, their own buttons do the job. -->
    {#if entries.length === 0}
      {#if buttonOn === null}
        <p>Wirecatch can also show a Download button over videos on web pages.</p>
      {:else}
        <p>
          {buttonOn
            ? 'A Download button also appears over videos on web pages.'
            : 'The Download button over videos on web pages is off.'}
          <Button variant="link" size="sm" class="text-small" onclick={onsettings}>
            {buttonOn ? 'Turn it off in settings' : 'Turn it on in settings'}
          </Button>
        </p>
      {/if}
    {/if}
  </div>
</section>
