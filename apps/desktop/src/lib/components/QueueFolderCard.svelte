<script lang="ts">
  // The queue editor's folder card: the queue saves to its own folder, or follows the folders Settings picks by file type (D94.1). Turned on with no folder yet, it asks for one at once and stays off when none is picked, so it is never on without a folder. A folder picked is kept while the switch is off.
  import {
    GLOBAL,
    ownSetting,
    ownValue,
    type PortablePath,
    type Setting,
  } from '@wirecatch/protocol';
  import { displayPath } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import FolderField from '../../ui/save/FolderField.svelte';
  import SwitchCard from './SwitchCard.svelte';

  let {
    folder,
    onchange,
  }: {
    folder: Setting<PortablePath>;
    onchange: (folder: Setting<PortablePath>) => void;
  } = $props();

  const app = getApp();
  let picked = $state<PortablePath | null>(ownValue((() => folder)()));
  const on = $derived(folder.source === 'own');

  async function choose(): Promise<void> {
    const chosen = await app.pickFolder(picked ?? undefined);
    if (chosen === null) return;
    picked = chosen;
    onchange(ownSetting(chosen));
  }

  function toggle(next: boolean) {
    if (!next) onchange(GLOBAL);
    else if (picked === null) {
      void choose();
    } else {
      onchange(ownSetting(picked));
    }
  }
</script>

<SwitchCard
  label="Save to its own folder"
  help="Everything this queue downloads is saved here, instead of in the folders by file type."
  inherited="Global: by file type"
  checked={on}
  onchange={toggle}
>
  <FolderField
    label="Folder"
    labelHidden
    folder={picked === null ? null : displayPath(picked)}
    onchoose={() => void choose()}
  />
</SwitchCard>
