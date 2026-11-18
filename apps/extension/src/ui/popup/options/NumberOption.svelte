<script lang="ts">
  // A number in its unit, or a switch with one under it where off means no limit (`limit`). The field shows the value divided by the unit's scale; what is sent is the value itself (D279).
  import { NumberInput, SettingRow, SettingSwitch } from '@wirecatch/ui';
  import { shownNumber, storedNumber, type ParsedOption } from '../../../lib/options';
  import type { PopupModel } from '../../../lib/popup/model.svelte';

  interface Props {
    model: PopupModel;
    option: Extract<ParsedOption, { kind: 'number' | 'limit' }>;
  }

  let { model, option }: Props = $props();

  const disabled = $derived(!model.editable || option.disabled !== null);
  const field = $derived(option.field);
  const unit = $derived(field.unit === '' ? undefined : field.unit);
  const value = $derived(option.value);

  function send(next: number | null): void {
    void model.setOption(option.id, { set: next });
  }

  function typed(shown: number | null): void {
    if (shown !== null && shown > 0) send(storedNumber(shown, field));
  }
</script>

{#if option.kind === 'number'}
  <SettingRow label={option.label} help={option.help ?? undefined}>
    {#snippet children(control)}
      <NumberInput
        {...control}
        class="w-24"
        {unit}
        min={field.min}
        max={field.max ?? undefined}
        step={field.step}
        value={value === null ? null : shownNumber(value, field)}
        {disabled}
        onchange={typed}
      />
    {/snippet}
  </SettingRow>
{:else}
  <!-- One block, so no hairline comes between the switch and its limit. -->
  <div>
    <SettingSwitch
      id="option-{option.id}"
      label={option.label}
      help={option.help ?? undefined}
      checked={value !== null}
      {disabled}
      onchange={(on: boolean) => {
        send(on ? option.onValue : null);
      }}
    />
    {#if value !== null}
      <div class="-mt-1 px-4 pb-3">
        <NumberInput
          label={option.label}
          class="w-36"
          {unit}
          min={field.min}
          max={field.max ?? undefined}
          step={field.step}
          value={shownNumber(value, field)}
          {disabled}
          onchange={typed}
        />
      </div>
    {/if}
  </div>
{/if}
