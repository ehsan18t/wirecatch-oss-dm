<script lang="ts">
  import type { ExtensionSettings, QueueId, QueueSummary } from '@wirecatch/protocol';
  import {
    Select,
    SettingRow,
    SettingsSection,
    SettingSwitch,
    type FieldControl,
    type SelectOption,
  } from '@wirecatch/ui';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import { SETTINGS, type SettingKey } from '../settings';

  interface Props {
    title: string;
    model: PopupModel;
    settings: ExtensionSettings;
    queues: readonly QueueSummary[];
    /** Whether a setting answers the search. */
    show: (key: SettingKey) => boolean;
  }

  let { title, model, settings, queues, show }: Props = $props();

  const editable = $derived(model.editable);
  /** The default queue was deleted in the app (or renamed away): say so instead of quietly showing another. */
  const queueMissing = $derived(
    queues.length > 0 && !queues.some((q) => q.id === settings.default_queue),
  );
  const queueOptions = $derived.by((): SelectOption<QueueId>[] => {
    if (queues.length === 0) return [{ value: settings.default_queue, label: 'Default queue' }];
    const missing = queueMissing
      ? [{ value: settings.default_queue, label: 'Missing queue (choose another)' }]
      : [];
    return [
      ...missing,
      ...queues.map((q) => ({ value: q.id, label: q.is_default ? `${q.name} (default)` : q.name })),
    ];
  });
</script>

<SettingsSection {title}>
  {#if show('captureEnabled')}
    <SettingSwitch
      id="capture-enabled"
      label={SETTINGS.captureEnabled.label}
      description={SETTINGS.captureEnabled.description}
      checked={settings.capture_enabled}
      disabled={!editable}
      onchange={(checked: boolean) => void model.update({ capture_enabled: checked })}
    />
  {/if}
  {#if show('askBeforeDownload')}
    <SettingSwitch
      id="ask-before"
      label={SETTINGS.askBeforeDownload.label}
      description={SETTINGS.askBeforeDownload.description}
      checked={settings.ask_before_download}
      disabled={!editable}
      onchange={(checked: boolean) => void model.update({ ask_before_download: checked })}
    />
  {/if}
  {#if show('defaultQueue')}
    <SettingRow
      label={SETTINGS.defaultQueue.label}
      description={SETTINGS.defaultQueue.description}
      error={queueMissing ? 'This queue no longer exists in Wirecatch. Choose another one.' : null}
    >
      {#snippet children(control: FieldControl)}
        <Select
          {...control}
          class="w-52"
          value={settings.default_queue}
          options={queueOptions}
          disabled={!editable}
          onchange={(queue: QueueId) => {
            if (queues.some((q) => q.id === queue)) void model.update({ default_queue: queue });
          }}
        />
      {/snippet}
    </SettingRow>
  {/if}
</SettingsSection>
