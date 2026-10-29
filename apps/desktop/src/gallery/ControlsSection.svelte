<script lang="ts" module>
  /** How long the sample Save button pretends to work. */
  const PRETEND_SAVE_MS = 1200;
</script>

<script lang="ts">
  import Download from '~icons/lucide/download';
  import FolderOpen from '~icons/lucide/folder-open';
  import Link from '~icons/lucide/link';
  import Pause from '~icons/lucide/pause';
  import Plus from '~icons/lucide/plus';
  import Settings from '~icons/lucide/settings';
  import Trash from '~icons/lucide/trash-2';
  import {
    Button,
    Checkbox,
    Field,
    IconButton,
    Input,
    NumberInput,
    RadioGroup,
    SegmentedControl,
    Select,
    Slider,
    Switch,
    Tabs,
    TextArea,
    type FieldControl,
    type Toaster,
  } from '@wirecatch/ui';
  import GallerySection from './GallerySection.svelte';
  import { BANDWIDTH_PERCENT, CONNECTIONS, SPEED_LIMIT } from './sample';
  import Specimen from './Specimen.svelte';

  let { toaster }: { toaster: Toaster } = $props();

  const QUALITIES = [
    { value: 'best', label: 'Best available', description: 'Up to 2160p, the largest file' },
    { value: '1080', label: '1080p' },
    { value: '720', label: '720p' },
    { value: 'audio', label: 'Audio only', disabled: true },
  ];

  let url = $state('https://example.com/file.zip');
  let links = $state('');
  let quality = $state('1080');
  let connections = $state<number | null>(CONNECTIONS.initial);
  let limit = $state<number | null>(null);
  let notify = $state(true);
  let startup = $state(false);
  let shutdown = $state(true);
  let clipboard = $state(false);
  let when = $state('now');
  let speed = $state(BANDWIDTH_PERCENT);
  let view = $state('all');
  let tab = $state('general');
  let pinned = $state(false);
  let saving = $state(false);

  function save() {
    saving = true;
    setTimeout(() => {
      saving = false;
      toaster.success('Settings saved');
    }, PRETEND_SAVE_MS);
  }
</script>

<GallerySection
  id="controls"
  title="Controls"
  description="Buttons, fields and choices. Each keeps a visible focus ring, follows its WAI-ARIA pattern, and falls back to system colours in Windows' high contrast themes."
>
  <Specimen title="Buttons" note="The main action is the only filled one">
    <Button variant="primary" icon={Plus}>New download</Button>
    <Button icon={FolderOpen}>Open folder</Button>
    <Button variant="ghost" icon={Pause}>Pause all</Button>
    <Button variant="danger" icon={Trash}>Remove</Button>
    <Button variant="link">Learn more</Button>
    <Button variant="primary" loading={saving} onclick={save}>Save</Button>
    <Button disabled tooltip="Nothing is selected">Retry</Button>
  </Specimen>

  <div class="grid gap-4 md:grid-cols-2">
    <Specimen title="Button sizes">
      <Button size="sm">Small</Button>
      <Button>Medium</Button>
      <Button size="lg" variant="primary" icon={Download}>Large</Button>
    </Specimen>
    <Specimen title="Icon buttons" note="Named by their label, which is also their tooltip">
      <IconButton icon={Settings} label="Settings" />
      <IconButton icon={Link} label="Copy link" variant="secondary" />
      <IconButton
        icon={Download}
        label="Pin the downloads view"
        active={pinned}
        aria-pressed={pinned}
        onclick={() => (pinned = !pinned)}
      />
      <IconButton icon={Plus} label="Add" variant="primary" />
      <IconButton icon={Trash} label="Remove" size="sm" />
      <IconButton icon={Pause} label="Pausing" loading />
    </Specimen>
  </div>

  <Specimen title="Fields" class="grid w-full items-start gap-4 md:grid-cols-2">
    <Field label="Address" description="A link to a file, a page or a video">
      {#snippet children(control: FieldControl)}
        <Input {...control} value={url} icon={Link} onchange={(value: string) => (url = value)} />
      {/snippet}
    </Field>
    <Field label="Folder" error="This folder cannot be written to.">
      {#snippet children(control: FieldControl)}
        <Input {...control} value="C:\Windows\System32" />
      {/snippet}
    </Field>
    <Field label="Quality">
      {#snippet children(control: FieldControl)}
        <Select
          {...control}
          value={quality}
          options={QUALITIES}
          onchange={(value: string) => (quality = value)}
        />
      {/snippet}
    </Field>
    <div class="grid grid-cols-2 gap-3">
      <Field label="Connections">
        {#snippet children(control: FieldControl)}
          <NumberInput
            {...control}
            value={connections}
            min={CONNECTIONS.min}
            max={CONNECTIONS.max}
            onchange={(value: number | null) => (connections = value)}
          />
        {/snippet}
      </Field>
      <Field label="Speed limit" description="Empty for no limit">
        {#snippet children(control: FieldControl)}
          <NumberInput
            {...control}
            value={limit}
            min={0}
            step={SPEED_LIMIT.step}
            unit="KB/s"
            optional
            onchange={(value: number | null) => (limit = value)}
          />
        {/snippet}
      </Field>
    </div>
    <Field label="Links" description="One per line" class="md:col-span-2">
      {#snippet children(control: FieldControl)}
        <TextArea
          {...control}
          value={links}
          onchange={(value: string) => (links = value)}
          mono
          placeholder="https://"
        />
      {/snippet}
    </Field>
  </Specimen>

  <div class="grid gap-4 md:grid-cols-2">
    <Specimen title="Switches and checkboxes" class="flex-col items-stretch">
      <Switch
        checked={notify}
        label="Notify when a download finishes"
        labelSide="start"
        onchange={(value: boolean) => (notify = value)}
      />
      <Switch
        checked={startup}
        label="Start with Windows"
        help="Opens in the tray, ready for the browser"
        labelSide="start"
        onchange={(value: boolean) => (startup = value)}
      />
      <Switch checked label="Syncing" labelSide="start" loading />
      <Checkbox
        checked={shutdown}
        label="Shut down when the queue is done"
        onchange={(value: boolean) => (shutdown = value)}
      />
      <Checkbox
        checked={clipboard}
        onchange={(value: boolean) => (clipboard = value)}
        label="Catch links copied to the clipboard"
        help="Only links to files"
      />
      <Checkbox checked indeterminate label="Some selected" />
      <Checkbox disabled label="Disabled" />
    </Specimen>
    <Specimen title="Choices" class="flex-col items-stretch gap-5">
      <RadioGroup
        label="Start the download"
        value={when}
        options={[
          { value: 'now', label: 'Now' },
          { value: 'queue', label: 'In the queue', description: 'After the downloads ahead of it' },
          { value: 'later', label: 'At a set time', disabled: true },
        ]}
        onchange={(value: string) => (when = value)}
      />
      <div class="flex flex-col gap-2">
        <span class="text-ui font-medium text-ink">Bandwidth for downloads: {speed}%</span>
        <Slider
          value={speed}
          label="Bandwidth for downloads"
          valueText={(value: number) => `${String(value)} percent`}
          oninput={(value: number) => (speed = value)}
        />
      </div>
      <SegmentedControl
        label="Show"
        value={view}
        block
        options={[
          { value: 'all', label: 'All' },
          { value: 'active', label: 'Active' },
          { value: 'finished', label: 'Finished' },
        ]}
        onchange={(value: string) => (view = value)}
      />
    </Specimen>
  </div>

  <Specimen title="Tabs" class="block w-full">
    <Tabs
      label="Settings"
      value={tab}
      tabs={[
        { value: 'general', label: 'General' },
        { value: 'files', label: 'Files' },
        { value: 'browser', label: 'Browser', count: 2 },
        { value: 'advanced', label: 'Advanced' },
      ]}
      onchange={(value: string) => (tab = value)}
    >
      {#snippet panel(value: string)}
        <p class="text-body text-ink-muted">The {value} settings would show here.</p>
      {/snippet}
    </Tabs>
  </Specimen>
</GallerySection>
