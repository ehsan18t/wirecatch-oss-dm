<script lang="ts">
  import Dialog from '../shell/Dialog.svelte';

  let { dismissible = true }: { dismissible?: boolean } = $props();
  let open = $state(false);
  let inner = $state(false);
</script>

<button type="button" onclick={() => (open = true)}>Remove</button>
<Dialog
  {open}
  {dismissible}
  title="Remove 3 downloads?"
  description="The files stay on disk."
  onclose={() => (open = false)}
>
  <label>Reason <input /></label>
  <button type="button" onclick={() => (inner = true)}>Details</button>
  <Dialog open={inner} title="Details" onclose={() => (inner = false)}>
    <button type="button">Inner action</button>
  </Dialog>
  {#snippet footer()}
    <button type="button" onclick={() => (open = false)}>Cancel</button>
    <button type="button">Remove</button>
  {/snippet}
</Dialog>
