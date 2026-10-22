<script lang="ts">
  // Removing an add-on: what stops working until it is installed again.
  import type { AddonId } from '@wirecatch/protocol';
  import { Button, Dialog } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';

  let { id }: { id: AddonId } = $props();
  const app = getApp();

  const addon = $derived(app.addons.list?.find((a) => a.id === id));
  const name = $derived(addon?.name.toLowerCase() ?? 'add-on');

  async function remove() {
    // Read before the dialog closes; its props go with it.
    const target = id;
    app.closeDialog();
    await app.addons.remove(target);
  }
</script>

<Dialog open role="alertdialog" title={`Remove the ${name}?`} onclose={() => app.closeDialog()}>
  <p class="text-ui text-ink-muted">
    Downloads from video sites, and videos whose sound and picture must be joined, stop working
    until you install it again. Your downloaded files stay.
  </p>

  {#snippet footer()}
    <Button data-autofocus onclick={() => app.closeDialog()}>Keep</Button>
    <Button variant="danger" onclick={remove}>Remove</Button>
  {/snippet}
</Dialog>
