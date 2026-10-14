<script lang="ts">
  // A placeholder for content still loading. It fades in only after the reveal delay, so a quick load never flashes it.
  import { cn } from '../lib/cn';

  interface Props {
    /** Its size and shape, as utilities (`h-4 w-32`, `size-8 rounded-full`). */
    class?: string | undefined;
  }

  let { class: className }: Props = $props();
</script>

<div class={cn('animate-reveal overflow-hidden rounded-sm', className)} aria-hidden="true">
  <div class="size-full bg-wash-strong motion-safe:animate-pulse"></div>
</div>
