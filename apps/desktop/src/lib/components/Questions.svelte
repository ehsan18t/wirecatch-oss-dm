<script lang="ts">
  // Questions the core asks without being asked first, one at a time and never over another
  // dialog: confirm a browser download, pick a video quality, install an add-on.
  import { getApp } from '$lib/state/app.svelte';
  import AddonPrompt from './AddonPrompt.svelte';
  import ConfirmCaptureForm from './ConfirmCaptureForm.svelte';
  import MediaChoiceForm from './MediaChoiceForm.svelte';

  const app = getApp();

  const capture = $derived(app.dialog === null && !app.pendingHidden ? app.pending[0] : undefined);
  const media = $derived(app.dialog === null && !capture ? app.mediaChoices[0] : undefined);
  const addon = $derived(
    app.dialog === null && !capture && !media ? (app.addonRequest ?? undefined) : undefined,
  );
</script>

{#if capture}
  {#key capture.id}
    <ConfirmCaptureForm {capture} waiting={app.pending.length - 1} />
  {/key}
{:else if media}
  {#key media.id}
    <MediaChoiceForm choice={media} waiting={app.mediaChoices.length - 1} />
  {/key}
{:else if addon}
  {#key addon.url}
    <AddonPrompt addon={addon.addon} url={addon.url} />
  {/key}
{/if}
