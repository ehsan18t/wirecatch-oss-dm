<script lang="ts">
  import type { MediaKind } from '@wirecatch/protocol';
  import type { MediaView } from '../lib/media/center';
  import type { MediaStatus } from '../lib/popup/model.svelte';
  import { formatSize } from '../lib/size';

  interface Props {
    /** The active tab's list; null while it is on its way. */
    view: MediaView | null;
    statuses: Record<string, MediaStatus>;
    /** The "Show download button on videos" setting; null before settings are known. */
    buttonOn: boolean | null;
    ondownload: (key: string) => void;
    /** The keyboard shortcut that downloads a page's media; null when none is set. */
    shortcut?: string | null;
  }

  let { view, statuses, buttonOn, ondownload, shortcut = null }: Props = $props();

  const keyboardHint = $derived(
    shortcut === null
      ? "To send a page's video or audio with a key press, set a shortcut in your browser's extension shortcuts."
      : `On a page, press ${shortcut} to send its video or audio to Wirecatch, or use the Download buttons in this list.`,
  );

  const BADGES: Record<MediaKind, string> = { hls: 'HLS', dash: 'DASH', direct: 'File', page: 'Page' };

  const buttonHint = $derived.by(() => {
    if (buttonOn === true) {
      return 'A Download button also appears over videos on web pages. Turn it off with "Show download button on videos" below.';
    }
    if (buttonOn === false) {
      return 'Turn on "Show download button on videos" below to get a Download button over videos on web pages.';
    }
    return 'Wirecatch can also show a Download button over videos on web pages.';
  });
</script>

<section class="media" aria-labelledby="media-title">
  <h2 id="media-title">Media on this page</h2>
  {#if view === null}
    <p class="help">Looking for video and audio on this page.</p>
  {:else if view.page === 'private'}
    <p class="help">Wirecatch does not take media from private windows.</p>
  {:else if view.page === 'other'}
    <p class="help">Open a web page with video or audio to see it here.</p>
  {:else if view.entries.length === 0}
    <p class="help">
      No video or audio found on this page yet. Start playing it, and it shows up in this list.
    </p>
    <p class="help">{buttonHint}</p>
  {:else}
    <ul class="list">
      {#each view.entries as entry (entry.key)}
        {@const status = statuses[entry.key]}
        <li class="item">
          <span class="badge" aria-hidden="true">{BADGES[entry.kind]}</span>
          <div class="text">
            <span class="title">{entry.title}</span>
            <span class="detail">
              {entry.detail}{entry.size === null ? '' : `, ${formatSize(entry.size)}`}
            </span>
            <p class="status status-{status?.tone ?? 'none'}" role="status">{status?.text ?? ''}</p>
          </div>
          <button
            type="button"
            class="button"
            aria-label="Download {entry.title}: {entry.detail}"
            aria-busy={status?.tone === 'busy'}
            onclick={() => ondownload(entry.key)}
          >
            Download
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  <p class="help">{keyboardHint}</p>
</section>

<style>
  .media {
    padding: 10px 0 8px;
    border-bottom: 1px solid var(--border);
  }

  h2 {
    margin: 0 0 4px;
    font-size: 13px;
    font-weight: 600;
  }

  .list {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .item {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 6px 0;
  }

  .item + .item {
    border-top: 1px solid var(--border);
  }

  .badge {
    flex: none;
    min-width: 40px;
    margin-top: 1px;
    padding: 1px 6px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    text-align: center;
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .title {
    font-weight: 600;
  }

  .detail {
    color: var(--muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .status {
    margin: 0;
    font-size: 12px;
    color: var(--muted);
  }

  .status-success {
    color: var(--ok);
  }

  .status-error {
    color: var(--error-text);
  }

  .item .button {
    flex: none;
  }
</style>
