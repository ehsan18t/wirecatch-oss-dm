<script lang="ts">
  import Dialog from '../shell/Dialog.svelte';
  import ToastRegion from '../shell/ToastRegion.svelte';
  import { provideToaster, Toaster } from '../shell/toaster.svelte';

  const toaster = new Toaster();
  provideToaster(toaster);
  let open = $state(false);
</script>

<button type="button" onclick={() => (open = true)}>Add a download</button>
<!-- Stand-ins for news from elsewhere in the app, such as a download that fails. -->
<button type="button" onclick={() => toaster.success('notes.txt finished')}>Earlier news</button>
<button type="button" onclick={() => toaster.danger('ubuntu.iso failed')}>Background failure</button
>
<Dialog {open} title="Add a download" onclose={() => (open = false)}>
  <button type="button">Start</button>
</Dialog>
<ToastRegion {toaster} />
