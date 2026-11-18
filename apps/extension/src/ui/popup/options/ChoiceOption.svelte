<script lang="ts">
  // One of a list, or several (`choices`), as a menu. Several are sent as what to add and remove, so an edit made at the same time in another browser stays (D279).
  import { MultiSelect, Select, SettingRow } from '@wirecatch/ui';
  import { menuOf, type MenuValue, type ParsedOption } from '../../../lib/options';
  import type { PopupModel } from '../../../lib/popup/model.svelte';

  interface Props {
    model: PopupModel;
    option: Extract<ParsedOption, { kind: 'choice' | 'choices' }>;
  }

  let { model, option }: Props = $props();

  /** A menu this long gets a search field. */
  const SEARCHABLE_FROM = 12;

  const disabled = $derived(!model.editable || option.disabled !== null);
  const menu = $derived(menuOf(option.choices));
  const chosen = $derived(
    option.kind === 'choices'
      ? option.value.filter((v): v is MenuValue => typeof v !== 'boolean')
      : [],
  );

  function pick(value: MenuValue): void {
    void model.setOption(option.id, { set: value });
  }

  function pickSeveral(values: MenuValue[]): void {
    const add = values.filter((v) => !chosen.includes(v));
    const remove = chosen.filter((v) => !values.includes(v));
    void model.setOption(option.id, { edit: { add, remove } });
  }
</script>

<SettingRow label={option.label} help={option.help ?? undefined} stacked>
  {#snippet children(control)}
    {#if option.kind === 'choice'}
      <Select
        {...control}
        class="w-full"
        value={typeof option.value === 'boolean' ? null : option.value}
        options={menu}
        searchable={option.choices.length > SEARCHABLE_FROM}
        {disabled}
        onchange={pick}
      />
    {:else}
      <MultiSelect
        {...control}
        class="w-full"
        placeholder={option.placeholder ?? undefined}
        searchPlaceholder={option.searchPlaceholder ?? undefined}
        values={chosen}
        options={menu}
        {disabled}
        onchange={pickSeveral}
      />
    {/if}
  {/snippet}
</SettingRow>
