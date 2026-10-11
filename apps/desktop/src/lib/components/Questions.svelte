<script lang="ts">
  // Questions the core asks without being asked first, one at a time and never over another
  // dialog: confirm a browser download, install an add-on, pick a video quality. The order
  // lives in `app.question`. Each form is keyed by its question, so one the core closes (answered
  // elsewhere, withdrawn by the browser) goes away and the next one opens fresh.
  import { getApp } from '$lib/state/app.svelte';
  import AddonPrompt from './AddonPrompt.svelte';
  import ConfirmCaptureForm from './ConfirmCaptureForm.svelte';
  import MediaChoiceForm from './MediaChoiceForm.svelte';

  const app = getApp();
  const question = $derived(app.question);
</script>

{#if question?.kind === 'capture'}
  {#key question.capture.id}
    <ConfirmCaptureForm capture={question.capture} waiting={question.waiting} />
  {/key}
{:else if question?.kind === 'addon'}
  {#key question.request.url}
    <AddonPrompt addon={question.request.addon} url={question.request.url} />
  {/key}
{:else if question?.kind === 'media'}
  {#key question.choice.id}
    <MediaChoiceForm choice={question.choice} waiting={question.waiting} />
  {/key}
{/if}
