<script lang="ts">
  import type { ExtensionSettings } from '@wirecatch/protocol';
  import {
    Input,
    NumberInput,
    Select,
    SettingRow,
    SettingsSection,
    type FieldControl,
    type SelectOption,
  } from '@wirecatch/ui';
  import { SIZE_UNITS, splitSize, toBytes, type SizeUnit } from '@wirecatch/units';
  import { formatFileTypes, parseFileTypes } from '../../lib/file-types';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import { SETTINGS, type SettingKey } from '../settings';

  interface Props {
    title: string;
    model: PopupModel;
    settings: ExtensionSettings;
    show: (key: SettingKey) => boolean;
  }

  let { title, model, settings, show }: Props = $props();

  const UNITS: SelectOption<SizeUnit>[] = SIZE_UNITS.map(({ unit }) => ({
    value: unit,
    label: unit,
  }));

  const editable = $derived(model.editable);
  const size = $derived(splitSize(settings.min_capture_size));
  /** What the person is typing in the file types box; kept while it has focus, so a change arriving from the app does not overwrite it. */
  let typesDraft = $state<string | null>(null);

  function commitFileTypes(value: string): void {
    const types = parseFileTypes(value);
    if (formatFileTypes(types) !== formatFileTypes(settings.capture_extensions)) {
      void model.update({ capture_extensions: types });
    }
  }

  function commitSize(value: number, unit: SizeUnit): void {
    const bytes = toBytes(value, unit);
    if (bytes !== null && bytes !== settings.min_capture_size) {
      void model.update({ min_capture_size: bytes });
    }
  }
</script>

<SettingsSection {title}>
  {#if show('fileTypes')}
    <SettingRow label={SETTINGS.fileTypes.label} description={SETTINGS.fileTypes.description}>
      {#snippet children(control: FieldControl)}
        <Input
          {...control}
          class="w-56"
          autocomplete="off"
          spellcheck="false"
          placeholder="Every file type"
          disabled={!editable}
          value={typesDraft ?? formatFileTypes(settings.capture_extensions)}
          onfocus={(event: FocusEvent & { currentTarget: HTMLInputElement }) => {
            typesDraft = event.currentTarget.value;
          }}
          onchange={(value: string) => {
            typesDraft = value;
          }}
          onblur={() => {
            if (typesDraft !== null) commitFileTypes(typesDraft);
            typesDraft = null;
          }}
          onkeydown={(event: KeyboardEvent & { currentTarget: HTMLInputElement }) => {
            if (event.key === 'Enter') commitFileTypes(event.currentTarget.value);
          }}
        />
      {/snippet}
    </SettingRow>
  {/if}
  {#if show('minSize')}
    <SettingRow label={SETTINGS.minSize.label} description={SETTINGS.minSize.description}>
      {#snippet children(control: FieldControl)}
        <NumberInput
          {...control}
          class="w-28"
          value={size.value}
          min={0}
          disabled={!editable}
          onchange={(value: number | null) => {
            commitSize(value ?? 0, size.unit);
          }}
        />
        <Select
          label="Size unit"
          class="w-20"
          value={size.unit}
          options={UNITS}
          disabled={!editable}
          onchange={(unit: SizeUnit) => {
            commitSize(size.value, unit);
          }}
        />
      {/snippet}
    </SettingRow>
  {/if}
</SettingsSection>
