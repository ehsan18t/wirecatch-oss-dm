<script lang="ts">
  import { getApp } from '$lib/state/app.svelte';
  import PaletteBody from './PaletteBody.svelte';

  const app = getApp();
</script>

{#if app.paletteOpen}
  <PaletteBody />
{/if}
