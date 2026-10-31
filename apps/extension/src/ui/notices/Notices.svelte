<script lang="ts">
  // The notices in a web page's frame (D65): the design system's toasts, with each notice's buttons, a close button, and Clear all while there are several. The frame sits at the page's bottom right and is only as big as they are, so the page around stays clickable; the page script around sizes it from what this reports.
  import { ToastRegion } from '@wirecatch/ui';
  import type { NoticesPage } from '@/lib/notices/page.svelte';

  let { page }: { page: NoticesPage } = $props();

  let stack = $state<HTMLElement | null>(null);

  // It reads the list, so it runs again whenever a notice comes or goes.
  $effect(() => page.changed());

  function measure(element: HTMLElement) {
    page.resized({
      wirecatchNoticesWidth: element.offsetWidth,
      wirecatchNoticesHeight: element.offsetHeight,
    });
  }

  $effect(() => {
    const element = stack;
    if (!element) return;
    const observer = new ResizeObserver(() => measure(element));
    observer.observe(element);
    return () => observer.disconnect();
  });

  // A notice that comes as another leaves keeps the size, which the observer never reports, so it would never be said to show.
  $effect(() => {
    if (stack && page.toaster.list.length > 0) measure(stack);
  });
</script>

<div bind:this={stack} class="ml-auto w-fit">
  <ToastRegion toaster={page.toaster} label="Wirecatch" inPlace />
</div>
