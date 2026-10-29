<script lang="ts">
  // Behind a modal layer (Dialog, the slide-over over a veil, the drawer) the page is inert, the window's toasts with it. The top layer shows them inside itself, where they can be read, pressed and heard, and the window's own stand aside meanwhile. It speaks for the window's toaster, which the root component provides (`provideToaster`).
  import ToastRegion from './ToastRegion.svelte';
  import { windowToaster } from './toaster.svelte';

  const toaster = windowToaster();
  // What was said before the layer opened is not said again.
  const before = toaster?.announcement?.id ?? null;
  const fresh = $derived(
    toaster?.announcement && toaster.announcement.id !== before ? toaster.announcement : null,
  );

  let layer = $state<symbol | null>(null);
  $effect(() => {
    if (!toaster) return;
    const opened = toaster.openLayer();
    layer = opened;
    return () => {
      toaster.closeLayer(opened);
    };
  });
</script>

{#if toaster && layer}<ToastRegion {toaster} {layer} announcement={fresh} />{/if}
