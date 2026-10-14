<script lang="ts">
  import SlideOver from '../shell/SlideOver.svelte';

  let { modal = false, docked = false }: { modal?: boolean; docked?: boolean } = $props();
  let open = $state(false);
</script>

<button type="button" onclick={() => (open = true)}>Show details</button>
<input aria-label="Search" />
<SlideOver {open} {modal} {docked} title="ubuntu.iso" onclose={() => (open = false)}>
  <button type="button">Open folder</button>
</SlideOver>
