<script lang="ts">
  // "Media on this page": the video and audio the extension saw on the active tab, each ready to send to Wirecatch, and the other ways to send it, with the button over videos switched right here (D61).
  import { Badge, Button, Card, Kbd } from '@wirecatch/ui';
  import type { MediaView } from '../../lib/media/center';
  import type { MediaStatus } from '../../lib/popup/model.svelte';
  import MediaItem from './MediaItem.svelte';

  interface Props {
    /** The active tab's list; null while it is on its way. */
    view: MediaView | null;
    statuses: Record<string, MediaStatus>;
    /** The keyboard shortcut that downloads a page's media; null when none is set. */
    shortcut: string | null;
    /** Opens the browser's page of extension shortcuts; null where no page can open it (Firefox). */
    openShortcuts: (() => void) | null;
    /** The "Show a download button on videos" setting; null while the app's settings are not known. */
    buttonOn: boolean | null;
    /** Turns that setting on or off in the app. */
    ontogglebutton: (on: boolean) => void;
    ondownload: (key: string) => void;
  }

  let { view, statuses, shortcut, openShortcuts, buttonOn, ontogglebutton, ondownload }: Props =
    $props();

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
    <p>
      {#if keys.length > 0}
        On a page, press <Kbd {keys} /> to send its video or audio to Wirecatch{entries.length > 0
          ? ', or use the Download buttons in this list.'
          : '.'}
      {:else}
        To send a page's video or audio with a key press, set a shortcut in your browser's extension
        shortcuts.
      {/if}
      {#if openShortcuts !== null}
        <Button variant="link" size="sm" class="text-small" onclick={openShortcuts}>
          {keys.length > 0 ? 'Change the shortcut' : 'Set a shortcut'}
        </Button>
      {/if}
    </p>
    <!-- The button over videos is taught while the list is empty; with items here, their own buttons do the job. -->
    {#if entries.length === 0 && buttonOn !== null}
      <p>
        {buttonOn
          ? 'A Download button also appears over videos on web pages.'
          : 'The Download button over videos on web pages is off.'}
        <Button
          variant="link"
          size="sm"
          class="text-small"
          onclick={() => {
            ontogglebutton(!buttonOn);
          }}
        >
          {buttonOn ? 'Turn it off' : 'Turn it on'}
        </Button>
      </p>
    {/if}
  </div>
</section>
