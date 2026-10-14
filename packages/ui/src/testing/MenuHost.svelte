<script lang="ts">
  import Menu from '../shell/Menu.svelte';
  import type { MenuEntry } from '../shell/menu';

  let { onselect }: { onselect: (id: string) => void } = $props();

  const entries: MenuEntry[] = [
    { id: 'pause', label: 'Pause', onselect: () => onselect('pause') },
    { id: 'retry', label: 'Retry', disabled: true, onselect: () => onselect('retry') },
    { id: 'line', kind: 'separator' },
    { id: 'copy', label: 'Copy link', shortcut: ['Ctrl', 'C'], onselect: () => onselect('copy') },
    { id: 'folder', label: 'Show in folder', onselect: () => onselect('folder') },
    { id: 'speed', label: 'Show speed', onselect: () => onselect('speed') },
    { id: 'remove', label: 'Remove', danger: true, onselect: () => onselect('remove') },
  ];
</script>

<Menu {entries} label="Download actions">
  {#snippet trigger(props)}
    <button type="button" {...props}>More</button>
  {/snippet}
</Menu>
<button type="button">Elsewhere</button>
