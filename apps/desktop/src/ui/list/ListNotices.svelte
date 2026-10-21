<script lang="ts">
  // Notices above the list: that the download list could not be saved, and one hint at a time, at the moment it is useful.
  import { Button, Callout, expand } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';
  import { HINT_TEXT, type HintId } from '$lib/state/hints.svelte';

  const app = getApp();

  let retrying = $state(false);

  const count = $derived(app.visible.length);
  const canReorder = $derived(app.sort.column === 'position');
  // One hint at a time, and only one that applies to what is on screen.
  const hint = $derived.by((): HintId | null => {
    if (count === 0) return null;
    const candidates: HintId[] = [];
    if (canReorder && count > 1) candidates.push('reorder');
    if (app.queues.list.length > 1) candidates.push('drag-to-queue');
    candidates.push('palette');
    return candidates.find((h) => app.hints.isOpen(h)) ?? null;
  });

  async function retry() {
    retrying = true;
    await app.retryStorage();
    retrying = false;
  }
</script>

{#if app.storageProblem !== null || hint !== null}
  <div class="flex shrink-0 flex-col gap-2 px-5 pb-3" transition:expand>
    {#if app.storageProblem}
      <div transition:expand>
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
    {#if hint}
      <div transition:expand>
        <Callout tone="info">
          {HINT_TEXT[hint]}
          {#snippet actions()}
            <Button size="sm" variant="ghost" onclick={() => app.hints.dismiss(hint)}>Got it</Button
            >
          {/snippet}
        </Callout>
      </div>
    {/if}
  </div>
{/if}
