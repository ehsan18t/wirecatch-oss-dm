<script lang="ts">
  // A file from the browser waiting for an answer: its name (editable), size, folder, queue and site, then Download now, Download later or Cancel. Enter in a field is Download now.
  import DownloadIcon from '~icons/lucide/download';
  import type { PendingCapture, QueueId } from '@wirecatch/protocol';
  import { formatBytes } from '@wirecatch/units';
  import { Button, Callout, textIfCut, tooltip } from '@wirecatch/ui';
  import { earlierNotice, hostOf, linkForDisplay } from '$lib/format';
  import { folderChosenBy, folderInQueue } from '$lib/queue-folders';
  import { queueLine } from '$lib/queue-reasons';
  import type { DownloadWindowState } from '$lib/window/download-window.svelte';
  import { enterSubmits } from '$lib/window/keys';
  import CategoryIcon from '../CategoryIcon.svelte';
  import FileNameField from '../save/FileNameField.svelte';
  import FolderField from '../save/FolderField.svelte';
  import QueueField from '../save/QueueField.svelte';
  import WindowFrame from './WindowFrame.svelte';
  import WindowOptions from './WindowOptions.svelte';

  let { win, capture }: { win: DownloadWindowState; capture: PendingCapture } = $props();
  const uid = $props.id();
  const formId = `${uid}-form`;

  // The form starts from the proposal and edits its own copy.
  const proposal = (() => ({ ...capture }))();
  let fileName = $state(proposal.file_name);
  let queue = $state<QueueId>(proposal.queue);
  /** The folder the person picked here; until then the queue's decides (D94.4). */
  let pickedFolder = $state<string | null>(null);
  /** The folder the app chooses: the queue's, or the one for the file type. */
  const appFolder = $derived(folderInQueue(win.queues, queue, proposal));
  const folder = $derived(pickedFolder ?? appFolder);
  const chosenBy = $derived(folderChosenBy(win.queues, queue, proposal, pickedFolder));
  /** Why the download goes to the queue shown: the core's reason, or the person's pick (D72.8). */
  const queueReason = $derived(
    queueLine(
      win.queues.find((q) => q.id === queue)?.settings.name ?? '',
      queue === proposal.queue ? proposal.queue_reason : { kind: 'picked' },
    ),
  );
  /** Which button sent the answer, for its spinner. */
  let sending = $state<'now' | 'later' | null>(null);

  const link = $derived(linkForDisplay(capture.url));

  async function changeFolder() {
    const picked = await win.changeFolder(folder);
    if (picked) pickedFolder = picked === appFolder ? null : picked;
  }

  async function answer(later: boolean) {
    if (win.busy) return;
    sending = later ? 'later' : 'now';
    await win.confirm({ fileName, folder: pickedFolder, queue }, later);
    sending = null;
  }
</script>

<WindowFrame
  title="Download this file?"
  description="Captured from your browser."
  error={win.error}
>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions (Enter in any field, checkboxes included, is Download now: see keys.ts) -->
  <form
    id={formId}
    class="flex flex-col gap-4"
    onkeydown={(event) => enterSubmits(event, () => void answer(false))}
    onsubmit={(event) => {
      event.preventDefault();
      void answer(false);
    }}
  >
    <div class="flex items-center gap-3">
      <CategoryIcon category={capture.category} />
      <div class="flex min-w-0 flex-col gap-0.5">
        <p class="flex min-w-0 flex-wrap gap-x-3 text-ui text-ink-muted tabular-nums">
          <span>{capture.size === null ? 'Size unknown' : formatBytes(capture.size)}</span>
          <span class="wrap-anywhere">from {hostOf(capture.page_url ?? capture.url)}</span>
        </p>
        <p class="truncate text-small text-ink-subtle" {@attach tooltip(() => textIfCut(link))}>
          {link}
        </p>
      </div>
    </div>

    {#if win.earlier}
      <div role="note">
        <Callout tone="info">{earlierNotice(win.earlier, win.clock.now)}</Callout>
      </div>
    {/if}

    <FileNameField value={fileName} onchange={(name: string) => (fileName = name)} />

    <FolderField
      {folder}
      onchoose={() => void changeFolder()}
      onreset={pickedFolder === null ? undefined : () => (pickedFolder = null)}
      note={chosenBy}
    />

    <QueueField
      queues={win.queues}
      value={queue}
      onchange={(chosen: QueueId | null) => (queue = chosen ?? queue)}
      description={queueReason}
    />

    <WindowOptions {win} files />
  </form>

  {#snippet footer()}
    <Button variant="ghost" disabled={win.busy} onclick={() => void win.discard()}>Cancel</Button>
    <Button
      disabled={win.busy && sending !== 'later'}
      loading={sending === 'later'}
      onclick={() => void answer(true)}>Download later</Button
    >
    <Button
      variant="primary"
      type="submit"
      form={formId}
      icon={DownloadIcon}
      data-autofocus
      disabled={win.busy && sending !== 'now'}
      loading={sending === 'now'}>Download now</Button
    >
  {/snippet}
</WindowFrame>
