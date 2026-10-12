<script lang="ts">
  import { getApp } from '$lib/state/app.svelte';
  import ToastItem from './ToastItem.svelte';

  const app = getApp();
</script>

<section class="toasts" aria-label="Notifications" aria-live="polite">
  {#each app.toasts.items as toast (toast.id)}
    <ToastItem {toast} />
  {/each}
</section>

<style>
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 20;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 8px;
    pointer-events: none;
  }

  .toasts > :global(*) {
    pointer-events: auto;
  }
</style>
