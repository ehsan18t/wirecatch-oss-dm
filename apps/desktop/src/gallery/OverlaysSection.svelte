<script lang="ts">
  import Copy from '~icons/lucide/copy';
  import Ellipsis from '~icons/lucide/ellipsis';
  import FolderOpen from '~icons/lucide/folder-open';
  import Gauge from '~icons/lucide/gauge';
  import Pause from '~icons/lucide/pause';
  import Trash from '~icons/lucide/trash-2';
  import {
    Badge,
    Button,
    Checkbox,
    ContextMenu,
    Dialog,
    IconButton,
    Menu,
    NumberInput,
    Popover,
    ProgressBar,
    SlideOver,
    type MenuEntry,
    type MenuTriggerProps,
    type PopoverTriggerProps,
    type Toaster,
  } from '@wirecatch/ui';
  import DownloadRowSample from './DownloadRowSample.svelte';
  import GallerySection from './GallerySection.svelte';
  import { ACTIVE_DOWNLOAD, FINISHED_DOWNLOAD, SPEED_LIMIT } from './sample';
  import Specimen from './Specimen.svelte';

  let { toaster }: { toaster: Toaster } = $props();

  let dialog = $state(false);
  let confirm = $state(false);
  let deleteFiles = $state(false);
  let details = $state(false);
  let detailsModal = $state(false);
  let limit = $state<number | null>(SPEED_LIMIT.initial);
  let showSpeed = $state(true);

  const entries: MenuEntry[] = $derived([
    {
      id: 'pause',
      label: 'Pause',
      icon: Pause,
      shortcut: ['Space'],
      onselect: () => toaster.info('Paused'),
    },
    {
      id: 'folder',
      label: 'Show in folder',
      icon: FolderOpen,
      onselect: () => toaster.info('Opened the folder'),
    },
    {
      id: 'copy',
      label: 'Copy link',
      icon: Copy,
      shortcut: ['Ctrl', 'C'],
      onselect: () => toaster.success('Link copied'),
    },
    { id: 'retry', label: 'Try again', disabled: true, onselect: () => undefined },
    { id: 'line', kind: 'separator' },
    {
      id: 'speed',
      kind: 'checkbox',
      label: 'Show speed',
      checked: showSpeed,
      onselect: () => (showSpeed = !showSpeed),
    },
    { id: 'line-2', kind: 'separator' },
    {
      id: 'remove',
      label: 'Remove',
      icon: Trash,
      danger: true,
      shortcut: ['Del'],
      onselect: () => (confirm = true),
    },
  ]);
</script>

<GallerySection
  id="overlays"
  title="Layers"
  description="Everything that floats: solid surfaces over the content, motion in and out, focus taken in and handed back, and Escape closing the nearest layer first."
>
  <div class="grid gap-4 md:grid-cols-2">
    <Specimen title="Dialogs">
      <Button onclick={() => (dialog = true)}>Open a dialog</Button>
      <Button variant="danger" onclick={() => (confirm = true)}>Remove 3 downloads</Button>
    </Specimen>
    <Specimen title="Toasts" note="Bottom right; hover holds them">
      <Button onclick={() => toaster.success('ubuntu.iso finished', { title: 'Download complete' })}
        >Success</Button
      >
      <Button
        onclick={() =>
          toaster.info('Added to the queue', {
            actions: [{ label: 'Show', run: () => undefined }],
          })}
      >
        With an action
      </Button>
      <Button onclick={() => toaster.warning('The link expires in 10 minutes')}>Warning</Button>
      <Button onclick={() => toaster.danger('The disk is full', { title: 'video.mp4' })}
        >Failure</Button
      >
    </Specimen>
    <Specimen title="Menus" note="Arrows, letters, Enter, Escape">
      <Menu {entries} label="Download actions">
        {#snippet trigger(props: MenuTriggerProps)}
          <IconButton {...props} icon={Ellipsis} label="More actions" variant="secondary" />
        {/snippet}
      </Menu>
      <Menu {entries} label="Download actions" align="end">
        {#snippet trigger(props: MenuTriggerProps)}
          <Button {...props}>Actions</Button>
        {/snippet}
      </Menu>
    </Specimen>
    <Specimen title="Popover and tooltip">
      <Popover label="Speed limit">
        {#snippet trigger(props: PopoverTriggerProps)}
          <Button {...props} icon={Gauge}>Speed limit</Button>
        {/snippet}
        {#snippet children(close: () => void)}
          <div class="flex w-60 flex-col gap-3">
            <p class="text-ui font-semibold text-ink">Speed limit for this download</p>
            <NumberInput
              value={limit}
              min={0}
              step={SPEED_LIMIT.step}
              unit="KB/s"
              optional
              label="Speed limit"
              onchange={(value: number | null) => (limit = value)}
            />
            <Button variant="primary" size="sm" onclick={close}>Done</Button>
          </div>
        {/snippet}
      </Popover>
      <Button
        variant="ghost"
        tooltip="Tooltips wait a moment, then follow the pointer from one control to the next at once."
      >
        Hover or focus me
      </Button>
    </Specimen>
  </div>

  <Specimen
    title="Context menu"
    note="Right-click the row, or focus one of its buttons and press Shift+F10"
  >
    <ContextMenu {entries} label="Download" class="w-full">
      <DownloadRowSample {...FINISHED_DOWNLOAD} />
    </ContextMenu>
  </Specimen>

  <Specimen title="Slide-over" note="Beside the content, or over a veil in a narrow window">
    <Button onclick={() => (details = true)}>Show details</Button>
    <Button onclick={() => (detailsModal = true)}>Show details over a veil</Button>
    <div class="relative h-80 w-full overflow-hidden rounded-md border border-line bg-base">
      <div class="p-3">
        <DownloadRowSample {...ACTIVE_DOWNLOAD} />
      </div>
      <SlideOver
        open={details}
        title={ACTIVE_DOWNLOAD.name}
        description="releases.ubuntu.com"
        onclose={() => (details = false)}
      >
        {#snippet meta()}<Badge tone="accent" size="sm" dot>Downloading</Badge>{/snippet}
        <div class="flex flex-col gap-4 text-ui">
          <ProgressBar value={ACTIVE_DOWNLOAD.value} label="Progress" />
          <dl class="grid grid-cols-2 gap-x-4 gap-y-2">
            <dt class="text-ink-muted">Size</dt>
            <dd class="text-ink">5.9 GB</dd>
            <dt class="text-ink-muted">Speed</dt>
            <dd class="text-ink">18.4 MB/s</dd>
            <dt class="text-ink-muted">Saved to</dt>
            <dd class="truncate text-ink">D:\Downloads</dd>
          </dl>
        </div>
        {#snippet footer()}
          <Button icon={Pause}>Pause</Button>
          <Button variant="ghost" icon={FolderOpen}>Show in folder</Button>
        {/snippet}
      </SlideOver>
    </div>
  </Specimen>
</GallerySection>

<SlideOver
  open={detailsModal}
  modal
  title="installer.msi"
  description="Finished today"
  onclose={() => (detailsModal = false)}
>
  <p class="text-body text-ink-muted">
    Over a veil, the slide-over is modal: Tab stays inside and the page behind waits.
  </p>
</SlideOver>

<Dialog
  open={dialog}
  title="New download"
  description="Paste a link, or several, one per line."
  onclose={() => (dialog = false)}
>
  <p class="text-body text-ink-muted">
    Dialogs settle in from slightly larger, blur what is behind them, start focus on their first
    control and hand it back when they close.
  </p>
  {#snippet footer()}
    <Button onclick={() => (dialog = false)}>Cancel</Button>
    <Button variant="primary" onclick={() => (dialog = false)}>Add</Button>
  {/snippet}
</Dialog>

<Dialog
  open={confirm}
  role="alertdialog"
  size="sm"
  title="Remove 3 downloads?"
  description="They leave the list. The files stay on disk unless you choose otherwise."
  onclose={() => (confirm = false)}
>
  <Checkbox
    checked={deleteFiles}
    label="Delete the files too"
    onchange={(value: boolean) => (deleteFiles = value)}
  />
  {#snippet footer()}
    <Button onclick={() => (confirm = false)}>Cancel</Button>
    <Button
      variant="danger"
      onclick={() => {
        confirm = false;
        toaster.success(deleteFiles ? 'Removed, with their files' : 'Removed from the list');
      }}
    >
      Remove
    </Button>
  {/snippet}
</Dialog>
