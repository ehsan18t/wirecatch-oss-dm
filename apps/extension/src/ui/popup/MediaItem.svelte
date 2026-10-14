<script lang="ts">
  // One item of "Media on this page". The title names the kind ("Audio file", "Video stream (HLS)"); the detail, usually the file name, is what tells items apart, so it gets the row's width and up to two lines, and the Download button is an icon.
  import { cn, IconButton, textIfCut, tooltip } from '@wirecatch/ui';
  import { formatBytes } from '@wirecatch/units';
  import Download from '~icons/lucide/download';
  import type { MediaEntry } from '../../lib/media/center';
  import type { MediaStatus } from '../../lib/popup/model.svelte';

  interface Props {
    entry: MediaEntry;
    /** What became of its Download button's last click. */
    status: MediaStatus | undefined;
    ondownload: () => void;
  }

  let { entry, status, ondownload }: Props = $props();

  const detail = $derived(
    entry.size === null ? entry.detail : `${entry.detail}, ${formatBytes(entry.size)}`,
  );
</script>

<li class="flex items-start gap-2 py-2.5 pr-2 pl-4">
  <div class="flex min-w-0 flex-1 flex-col">
    <span
      class="truncate text-ui font-medium text-ink"
      {@attach tooltip(() => textIfCut(entry.title))}>{entry.title}</span
    >
    <span
      class="line-clamp-2 text-small wrap-anywhere text-ink-muted"
      {@attach tooltip(() => textIfCut(detail))}>{detail}</span
    >
    <p
      role="status"
      class={cn(
        'text-small',
        status?.tone === 'success' && 'text-success',
        status?.tone === 'error' && 'text-danger',
        (status === undefined || status.tone === 'busy') && 'text-ink-muted',
      )}
    >
      {status?.text ?? ''}
    </p>
  </div>
  <IconButton
    icon={Download}
    variant="secondary"
    label="Download {entry.title}: {entry.detail}"
    tooltip="Download"
    loading={status?.tone === 'busy'}
    onclick={ondownload}
  />
</li>
