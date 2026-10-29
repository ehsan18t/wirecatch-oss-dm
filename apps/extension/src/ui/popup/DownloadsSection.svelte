<script lang="ts">
  // The app's download settings someone may want to change while browsing (D61), folded under one line that says what they are set to: the queue browser downloads join, a limit on the total speed, and how many downloads run at once. Every change is saved in the app.
  import {
    MAX_CONCURRENT_DOWNLOADS,
    type BrowserControls,
    type ExtensionSettings,
    type QueueId,
    type QueueSummary,
  } from '@wirecatch/protocol';
  import {
    Disclosure,
    NumberInput,
    Select,
    SettingRow,
    SettingSwitch,
    type SelectOption,
  } from '@wirecatch/ui';
  import {
    FIRST_TOTAL_SPEED_LIMIT,
    formatRate,
    inUnit,
    SPEED_FIELD,
    toBytes,
  } from '@wirecatch/units';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import { SETTINGS } from '../settings';

  interface Props {
    model: PopupModel;
    settings: ExtensionSettings;
    /** Null for an app from before the popup could show them: only the queue shows. */
    controls: BrowserControls | null;
    queues: QueueSummary[];
  }

  let { model, settings, controls, queues }: Props = $props();

  const editable = $derived(model.editable);
  const queueOptions = $derived<SelectOption<QueueId>[]>(
    queues.map((q) => ({ value: q.id, label: q.name })),
  );
  const queueName = $derived(queues.find((q) => q.id === settings.default_queue)?.name ?? null);
  // The queue last: a long name is the part that gets cut.
  const summary = $derived(
    [
      controls === null
        ? null
        : controls.global_speed_limit === null
          ? 'No speed limit'
          : formatRate(controls.global_speed_limit),
      controls === null ? null : `${String(controls.max_concurrent_downloads)} at once`,
      queueName,
    ]
      .filter((part) => part !== null)
      .join(' · '),
  );
</script>

<Disclosure variant="card" label="Downloads" {summary}>
  <SettingRow label={SETTINGS.defaultQueue.label} help={SETTINGS.defaultQueue.help} stacked>
    {#snippet children(control)}
      <Select
        {...control}
        class="w-full"
        value={settings.default_queue}
        options={queueOptions}
        disabled={!editable}
        onchange={(queue: QueueId) => void model.update({ default_queue: queue })}
      />
    {/snippet}
  </SettingRow>
  {#if controls !== null}
    {const limit = $derived(controls.global_speed_limit)}
    <!-- One block, so no hairline comes between the switch and its limit. -->
    <div>
      <SettingSwitch
        id="speed-limit"
        label={SETTINGS.speedLimit.label}
        help={SETTINGS.speedLimit.help}
        checked={limit !== null}
        disabled={!editable}
        onchange={(on: boolean) =>
          void model.updateControls({ global_speed_limit: on ? FIRST_TOTAL_SPEED_LIMIT : null })}
      />
      {#if limit !== null}
        <div class="-mt-1 px-4 pb-3">
          <NumberInput
            label="Global speed limit"
            class="w-36"
            unit="MB/s"
            min={SPEED_FIELD.MB.min}
            step={SPEED_FIELD.MB.step}
            value={inUnit(limit, 'MB')}
            disabled={!editable}
            onchange={(value: number | null) => {
              const bytes = value === null ? null : toBytes(value, 'MB');
              if (bytes !== null && bytes > 0)
                void model.updateControls({ global_speed_limit: bytes });
            }}
          />
        </div>
      {/if}
    </div>
    <SettingRow label={SETTINGS.concurrent.label} help={SETTINGS.concurrent.help}>
      {#snippet children(control)}
        <NumberInput
          {...control}
          class="w-24"
          min={1}
          max={MAX_CONCURRENT_DOWNLOADS}
          value={controls.max_concurrent_downloads}
          disabled={!editable}
          onchange={(value: number | null) => {
            if (value !== null) void model.updateControls({ max_concurrent_downloads: value });
          }}
        />
      {/snippet}
    </SettingRow>
  {/if}
</Disclosure>
