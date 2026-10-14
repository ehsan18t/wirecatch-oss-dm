<script lang="ts">
  // Two live regions for toast news, always on the page while their owner is (a region must exist before its text arrives): failures interrupt, the rest wait their turn.
  import type { Announcement } from '../shell/toaster.svelte';

  let { announcement }: { announcement: Announcement | null } = $props();
</script>

{#each [true, false] as assertive (assertive)}
  {@const current = announcement?.assertive === assertive ? announcement : null}
  <div
    class="sr-only"
    role={assertive ? 'alert' : 'status'}
    aria-live={assertive ? 'assertive' : 'polite'}
  >
    {#if current}
      {#key current.id}<p>{current.text}</p>{/key}
    {/if}
  </div>
{/each}
