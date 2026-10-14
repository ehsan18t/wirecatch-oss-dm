<script lang="ts">
  // Shown when the person took the extension's access to websites away (Firefox allows it): without it, downloads stay in the browser.
  import { Button, Callout } from '@wirecatch/ui';

  interface Props {
    heading?: 'h2' | 'h3';
    onallow: () => void;
  }

  let { heading = 'h2', onallow }: Props = $props();
</script>

<Callout tone="warning">
  <svelte:element this={heading} class="font-semibold text-ink">
    Wirecatch cannot read website cookies.
  </svelte:element>
  <p>Downloads stay in the browser until you allow access to websites.</p>
  <div class="mt-2.5 flex flex-wrap gap-2">
    <Button size="sm" variant="primary" onclick={onallow}>Allow access</Button>
  </div>
</Callout>
