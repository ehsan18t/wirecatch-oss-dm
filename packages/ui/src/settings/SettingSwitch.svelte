<script lang="ts">
  // A setting that is on or off, in a SettingsSection: the switch at the row's end with its name and description before it, as Windows 11's settings show one. The switch is named by its label, so the row needs no label of its own.
  import Switch from '../primitives/Switch.svelte';

  interface Props {
    label: string;
    description?: string | undefined;
    checked: boolean;
    disabled?: boolean;
    /** Waiting for the change to land. */
    loading?: boolean;
    id?: string | undefined;
    onchange?: (checked: boolean) => void;
  }

  let {
    label,
    description,
    checked,
    disabled = false,
    loading = false,
    id,
    onchange,
  }: Props = $props();
</script>

<div class="px-4 py-3">
  <Switch labelSide="start" {label} {description} {checked} {disabled} {loading} {id} {onchange} />
</div>
