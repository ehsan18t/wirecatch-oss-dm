<script lang="ts">
  // A new link for a download whose link stopped working: pasted from the page it came from, it continues where it stopped.
  import type { DownloadId } from '@wirecatch/protocol';
  import { Button, Dialog, Field, Input, type FieldControl } from '@wirecatch/ui';
  import { extractUrls, hostOf } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';

  let { id }: { id: DownloadId } = $props();
  const app = getApp();
  const formId = $props.id();

  const row = $derived(app.downloads.get(id));
  let url = $state('');
  const found = $derived(extractUrls(url)[0]);
  let busy = $state(false);

  async function save() {
    if (!found || busy) return;
    busy = true;
    const saved = await app.actions.refreshLink(id, found);
    busy = false;
    if (saved) {
      app.toasts.notify('New link saved. The download continues where it stopped.');
      app.closeDialog();
    }
  }
</script>

<Dialog
  open
  title="Get a new link"
  description="Links from some sites stop working after a while. Start the download again on its page, copy the new link and paste it here. What is already downloaded is kept."
  onclose={() => app.closeDialog()}
>
  <form
    id={formId}
    class="flex flex-col gap-4 pb-1"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    {#if row}
      <p class="flex flex-col gap-0.5">
        <span class="text-ui font-medium break-all text-ink">{row.file_name}</span>
        <span class="text-small text-ink-muted">from {hostOf(row.url)}</span>
      </p>
    {/if}
    <Field
      label="New link"
      error={url.trim() !== '' && !found ? 'Paste the whole link, starting with https://.' : null}
    >
      {#snippet children(control: FieldControl)}
        <Input
          {...control}
          type="url"
          placeholder="https://"
          autocomplete="off"
          spellcheck="false"
          value={url}
          onchange={(value: string) => (url = value)}
        />
      {/snippet}
    </Field>
  </form>
  {#snippet footer()}
    <Button onclick={() => app.closeDialog()}>Cancel</Button>
    <Button variant="primary" type="submit" form={formId} disabled={!found} loading={busy}
      >Use this link</Button
    >
  {/snippet}
</Dialog>
