<script lang="ts">
  // Where a download is saved: the folder, or what decides it while none is chosen, with Change and, when the owner offers it, Use default. The owner asks for the folder and keeps it. The Add dialog uses it, and the download window's save question can.
  import FolderIcon from '~icons/lucide/folder';
  import { Button, Icon } from '@wirecatch/ui';

  interface Props {
    /** The folder chosen; null while the default decides. */
    folder: string | null;
    /** What decides the folder while none is chosen. */
    fallback?: string;
    label?: string;
    /** Asks for another folder. */
    onchoose: () => void;
    /** Goes back to the default; offered while a folder is chosen. */
    onreset?: (() => void) | undefined;
  }

  let {
    folder,
    fallback = 'By file type, as set in Settings',
    label = 'Save to',
    onchoose,
    onreset,
  }: Props = $props();

  const id = $props.id();
  const labelId = `${id}-label`;
  const folderId = `${id}-folder`;
  const changeId = `${id}-change`;

  function reset() {
    onreset?.();
    // Use default goes with the folder; the focus stays in the field rather than falling to the page.
    document.getElementById(changeId)?.focus();
  }
</script>

<div role="group" aria-labelledby={labelId} class="flex min-w-0 flex-col gap-1.5">
  <span id={labelId} class="text-ui font-medium text-ink">{label}</span>
  <div class="flex flex-wrap items-center gap-2">
    <p
      id={folderId}
      class="flex min-h-control min-w-0 flex-1 basis-64 items-center gap-2 rounded-sm border border-line-strong/quiet bg-field px-2.5 py-1 text-ui text-ink"
    >
      <Icon icon={FolderIcon} size="sm" class="shrink-0 text-ink-subtle" />
      <span class="min-w-0 wrap-anywhere">{folder ?? fallback}</span>
    </p>
    <div class="flex shrink-0 gap-2">
      <Button id={changeId} aria-describedby={folderId} onclick={onchoose}>Change</Button>
      {#if folder !== null && onreset}
        <Button variant="ghost" onclick={reset}>Use default</Button>
      {/if}
    </div>
  </div>
</div>
