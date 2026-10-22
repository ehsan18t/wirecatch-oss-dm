<script lang="ts">
  // A download window's shell (D23, D36): the frameless title bar with its close button, over the content layer. Escape closes the window, and the window is as tall as the title bar and what its content measures.
  import type { Snippet } from 'svelte';
  import { AppMark, TitleBar } from '@wirecatch/ui';
  import { getFitWindow, setFitWindow } from '$lib/window/fit';
  import { closeOnlyControls } from '$lib/window/window-controls';

  let { name, onclose, children }: { name: string; onclose: () => void; children: Snippet } =
    $props();

  const controls = closeOnlyControls(() => onclose());

  let root = $state<HTMLDivElement>();
  let barBox = $state<readonly ResizeObserverSize[]>();

  // The frame below measures what it needs; the window needs the title bar on top of that, and asks again when the bar alone changes height (a larger text size).
  const fitWindow = getFitWindow();
  let frameNeeds = $state(0);
  if (fitWindow) setFitWindow((height) => (frameNeeds = height));
  $effect(() => {
    if (fitWindow && frameNeeds > 0)
      fitWindow(Math.ceil(frameNeeds + (barBox?.[0]?.blockSize ?? 0)));
  });

  // Escape closes the window. The sample page shows several windows in one page, so only keys from inside this one count; the window's own controls are what take them.
  function onkeydown(event: KeyboardEvent) {
    if (event.key !== 'Escape' || event.defaultPrevented) return;
    if (!(event.target instanceof Node) || !root?.contains(event.target)) return;
    event.preventDefault();
    onclose();
  }
</script>

<svelte:window {onkeydown} />

<div bind:this={root} class="flex h-full flex-col">
  <div bind:borderBoxSize={barBox} class="shrink-0 border-b border-line">
    <TitleBar title={name} {controls} closeOnly>
      {#snippet icon()}
        <AppMark />
      {/snippet}
    </TitleBar>
  </div>
  <div class="min-h-0 flex-1 bg-layer">
    {@render children()}
  </div>
</div>
