<script lang="ts">
  import Popover from '../shell/Popover.svelte';
</script>

<Popover label="Speed limit">
  {#snippet trigger(props)}
    <button type="button" {...props}>Limit</button>
  {/snippet}
  {#snippet children(close)}
    <input aria-label="Kilobytes per second" />
    <button type="button" onclick={close}>Done</button>
  {/snippet}
</Popover>
<button type="button">Elsewhere</button>
