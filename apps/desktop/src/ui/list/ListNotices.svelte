<script lang="ts">
  // The notice above the list that the download list could not be saved: it stays until dealt with. Teaching lives on the controls themselves (D48), not in banners here.
  import { Button, Callout, expand } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';

  const app = getApp();

  let retrying = $state(false);

  async function retry() {
    retrying = true;
    await app.retryStorage();
    retrying = false;
  }
</script>

{#if app.storageProblem}
  <div class="shrink-0 px-5 pb-3" transition:expand>
    <Callout tone="danger" title="{app.name} could not save its download list" live="assertive">
      {app.storageProblem} Recent changes may be lost when {app.name} closes.
      {#snippet actions()}
        <Button size="sm" loading={retrying} onclick={retry}>Try again</Button>
        <Button size="sm" variant="ghost" onclick={() => app.dismissStorageProblem()}
          >Dismiss</Button
        >
      {/snippet}
    </Callout>
  </div>
{/if}
