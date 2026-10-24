<script lang="ts">
  import Disclosure from '../primitives/Disclosure.svelte';

  let { variant }: { variant?: 'inline' | 'card' } = $props();
</script>

<Disclosure label="Subtitles" summary="English · chapters" {variant}>
  <button type="button">English</button>
</Disclosure>
