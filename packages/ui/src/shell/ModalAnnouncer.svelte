<script lang="ts">
  // Behind a modal layer (Dialog, the slide-over over a veil, the drawer) the page is inert, the toasts' live regions with it. This repeats, inside the layer, news that arrives while it is open, so a download that fails in the background is still heard. It speaks for the window's toaster, which the root component provides (`provideToaster`).
  import LiveRegions from '../internal/LiveRegions.svelte';
  import { windowToaster } from './toaster.svelte';

  const toaster = windowToaster();
  // What was said before the layer opened is not said again.
  const before = toaster?.announcement?.id ?? null;
  const fresh = $derived(
    toaster?.announcement && toaster.announcement.id !== before ? toaster.announcement : null,
  );
</script>

{#if toaster}<LiveRegions announcement={fresh} />{/if}
