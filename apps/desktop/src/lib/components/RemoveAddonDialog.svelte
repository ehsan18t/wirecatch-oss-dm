<script lang="ts">
  import { getApp } from '$lib/state/app.svelte';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';

  let { id }: { id: string } = $props();
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

<Dialog title={`Remove the ${name}?`} onclose={() => app.closeDialog()} size="narrow">
  <p class="text">
    Downloads from video sites, and videos whose sound and picture must be joined, stop working
    until you install it again. Your downloaded files stay.
  </p>

  {#snippet footer()}
    <Button data-autofocus onclick={() => app.closeDialog()}>Keep</Button>
    <Button variant="danger" onclick={remove}>Remove</Button>
  {/snippet}
</Dialog>

<style>
  .text {
    margin: 0;
    color: var(--ink-2);
    line-height: 1.5;
  }
</style>
