<script lang="ts">
  import ContextMenu from '../shell/ContextMenu.svelte';
  import type { MenuEntry } from '../shell/menu';

  let {
    onselect,
    onopen,
  }: { onselect: (id: string) => void; onopen?: (target: Element) => boolean | undefined } =
    $props();

  const entries: MenuEntry[] = [
    { id: 'open', label: 'Open', onselect: () => onselect('open') },
    { id: 'show', label: 'Show in folder', onselect: () => onselect('show') },
  ];
</script>

<ContextMenu {entries} label="Download" {onopen}>
  <div role="grid" tabindex="0" aria-label="Files" aria-activedescendant="file-2">
    <div role="row" id="file-1"><span role="gridcell">a.zip</span></div>
    <div role="row" id="file-2"><span role="gridcell">b.zip</span></div>
  </div>
  <button type="button">ubuntu.iso</button>
</ContextMenu>
<button type="button">After</button>
