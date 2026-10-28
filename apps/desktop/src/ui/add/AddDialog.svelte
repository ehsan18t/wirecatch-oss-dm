<script lang="ts">
  // Adding downloads by hand: links pasted or typed, one per line, then where they are saved, in which queue (Automatic, where the claims send each, unless one is picked), and whether they start now. The form is state (AddDraft), so links that reach the window while it is open join it. Opened with links, it starts on Download, so Enter sends them as they are; opened empty, on the links box.
  import DownloadIcon from '~icons/lucide/download';
  import type { QueueId } from '@wirecatch/protocol';
  import { Button, Callout, Dialog, Field, type FieldControl, Kbd, TextArea } from '@wirecatch/ui';
  import { fileNameFromUrl } from '$lib/format';
  import { type AddDraft, linksHint } from '$lib/state/add-draft.svelte';
  import { getApp } from '$lib/state/app.svelte';
  import FileNameField from '../save/FileNameField.svelte';
  import FolderField from '../save/FolderField.svelte';
  import QueueField from '../save/QueueField.svelte';

  let { draft }: { draft: AddDraft } = $props();

  const app = getApp();
  const id = $props.id();
  const formId = `${id}-form`;

  /** Rows the links box has for one link or none: room to paste or type a few by hand. */
  const LINK_ROWS = 3;
  /** The links box grows with the links up to this many rows, then scrolls. */
  const LINK_ROWS_MAX = 8;

  const count = $derived(draft.links.urls.length);
  const rows = $derived(count > 1 ? Math.min(LINK_ROWS_MAX, count + 1) : LINK_ROWS);
  /** Where Automatic sends the links, asked again a moment after they stop changing. */
  let placement = $state('');
  const PLACEMENT_WAIT_MS = 250;
  $effect(() => {
    const { urls } = draft.links;
    if (draft.queue !== null) return;
    let current = true;
    const timer = setTimeout(() => {
      void app.queues.placement(urls).then((line) => {
        if (current) placement = line;
      });
    }, PLACEMENT_WAIT_MS);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  });
  const queueHint = $derived(draft.queue === null ? placement : 'For this add only.');

  /** Which button sent the links, for its spinner. */
  let sending = $state<'now' | 'paused' | null>(null);

  async function submit(start: boolean) {
    // The send in flight keeps its spinner, and the focus stays put.
    if (draft.busy) return;
    sending = start ? 'now' : 'paused';
    const done = await draft.submit(start);
    sending = null;
    if (!done) showLinks();
  }

  /** Something is wrong with the links: the focus goes to them, and a reader hears the field with its message. Already there, moving it says nothing, so the dialog says the message. */
  function showLinks() {
    const box = document.getElementById(formId)?.querySelector('textarea');
    if (!box) return;
    if (document.activeElement === box) {
      if (draft.invalid !== null) app.toasts.toaster.announce(draft.invalid);
    } else {
      box.focus();
    }
  }

  function onLinksKey(event: KeyboardEvent) {
    if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void submit(true);
    }
  }
</script>

<Dialog
  open
  size="lg"
  title={count > 1 ? `${String(count)} new downloads` : 'New download'}
  onclose={() => app.closeDialog()}
>
  <form
    id={formId}
    class="flex flex-col gap-4 pb-1"
    onsubmit={(event) => {
      event.preventDefault();
      void submit(true);
    }}
  >
    {#if draft.problem}
      <Callout tone="danger" live="assertive" title={draft.problem.title}
        >{draft.problem.message}</Callout
      >
    {/if}

    <Field label="Links" description={linksHint(draft.links)} error={draft.invalid}>
      {#snippet children(control: FieldControl)}
        <TextArea
          {...control}
          mono
          {rows}
          value={draft.text}
          onchange={(text: string) => draft.edit(text)}
          onkeydown={onLinksKey}
          placeholder="https://example.com/file.zip"
          spellcheck="false"
          autocomplete="off"
        />
      {/snippet}
    </Field>

    <div class="flex flex-wrap gap-4">
      <!-- There before a link is, so pasting one does not move the form; several links each keep their own name. -->
      {#if count <= 1}
        <FileNameField
          class="flex-1 basis-64"
          value={draft.fileName}
          onchange={(name: string) => (draft.fileName = name)}
          placeholder={draft.single ? fileNameFromUrl(draft.single) : ''}
          description="Leave empty for the name the server gives."
        />
      {/if}
      <QueueField
        class="flex-1 basis-48"
        queues={app.queues.list}
        automatic
        value={draft.queue}
        description={queueHint}
        onchange={(queue: QueueId | null) => (draft.queue = queue)}
      />
    </div>

    <FolderField
      folder={draft.folder}
      onchoose={() => void draft.chooseFolder()}
      onreset={() => {
        draft.useDefaultFolder();
      }}
    />
  </form>

  {#snippet footer()}
    <span class="mr-auto flex items-center gap-1.5 text-small text-ink-muted">
      <Kbd keys={['Ctrl', 'Enter']} /> to download
    </span>
    <Button
      disabled={draft.busy && sending !== 'paused'}
      loading={sending === 'paused'}
      onclick={() => void submit(false)}>Add paused</Button
    >
    <Button
      variant="primary"
      type="submit"
      form={formId}
      icon={DownloadIcon}
      disabled={draft.busy && sending !== 'now'}
      loading={sending === 'now'}
      data-autofocus={draft.fromPaste ? '' : undefined}
    >
      {count > 1 ? `Download ${String(count)}` : 'Download'}
    </Button>
  {/snippet}
</Dialog>
