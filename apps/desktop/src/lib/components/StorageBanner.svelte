<script lang="ts">
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import { getApp } from '$lib/state/app.svelte';
  import Button from './Button.svelte';
  import Icon from './Icon.svelte';

  const app = getApp();
  let busy = $state(false);

  async function retry() {
    busy = true;
    await app.retryStorage();
    busy = false;
  }
</script>

{#if app.storageProblem}
  <div class="banner" role="alert">
    <Icon icon={TriangleAlert} size="md" />
    <div class="text">
      <p class="title">Wirecatch could not save its download list</p>
      <p>
        {app.storageProblem} Recent changes may be lost when Wirecatch closes.
      </p>
    </div>
    <div class="actions">
      <Button small disabled={busy} onclick={retry}>Try again</Button>
      <Button variant="ghost" small onclick={() => app.dismissStorageProblem()}>Dismiss</Button>
    </div>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    margin: 10px 20px 0;
    padding: 10px 12px;
    border: 1px solid var(--danger);
    border-radius: var(--radius-m);
    background: var(--danger-soft);
    color: var(--ink);
    font-size: var(--text-sm);
  }

  .banner > :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--danger);
  }

  .text {
    flex: 1;
    min-width: 0;
  }

  p {
    margin: 0;
    line-height: 1.45;
  }

  .title {
    font-weight: 600;
  }

  .actions {
    display: flex;
    flex: none;
    gap: 4px;
  }
</style>
