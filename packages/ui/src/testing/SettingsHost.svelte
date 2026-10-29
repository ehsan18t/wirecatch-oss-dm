<script lang="ts">
  import Input from '../primitives/Input.svelte';
  import type { FieldControl } from '../primitives/field-control';
  import SettingRow from '../settings/SettingRow.svelte';
  import SettingsSection from '../settings/SettingsSection.svelte';
  import SettingSwitch from '../settings/SettingSwitch.svelte';

  let { error = null }: { error?: string | null } = $props();
  let on = $state(false);
  let folder = $state('Downloads');
</script>

<SettingsSection title="Saving" description="Where finished files go.">
  <SettingSwitch
    label="Ask where to save"
    help="Choose a folder for each file."
    checked={on}
    onchange={(next: boolean) => (on = next)}
  />
  <SettingRow label="Folder" help="New downloads go here." {error}>
    {#snippet children(control: FieldControl)}
      <Input {...control} value={folder} onchange={(next: string) => (folder = next)} />
    {/snippet}
  </SettingRow>
  <SettingRow label="Version" help="Whether a newer version is out." labelled={false}>
    {#snippet children(control: FieldControl)}
      <button type="button" aria-describedby={control['aria-describedby']}>Check for updates</button
      >
    {/snippet}
  </SettingRow>
</SettingsSection>
