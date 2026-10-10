<script lang="ts">
  import { getApp } from '$lib/state/app.svelte';
  import ConfirmCaptureForm from './ConfirmCaptureForm.svelte';

  const app = getApp();
  const current = $derived(app.dialog === null && !app.pendingHidden ? app.pending[0] : undefined);
</script>

{#if current}
  {#key current.id}
    <ConfirmCaptureForm capture={current} waiting={app.pending.length - 1} />
  {/key}
{/if}
