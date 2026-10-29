<script lang="ts">
  // A setting that is on or off, in a SettingsSection: its name, then the switch at the row's end, as Windows 11's settings show one. The switch is named by its label, so the row needs no label of its own.
  import Switch from '../primitives/Switch.svelte';

  interface Props {
    label: string;
    /** What the setting does, behind an info icon after the label (D84). */
    help?: string | undefined;
    checked: boolean;
    disabled?: boolean;
    /** Waiting for the change to land. */
    loading?: boolean;
    id?: string | undefined;
    onchange?: (checked: boolean) => void;
  }

  let { label, help, checked, disabled = false, loading = false, id, onchange }: Props = $props();
</script>

<div class="px-4 py-3">
  <Switch labelSide="start" {label} {help} {checked} {disabled} {loading} {id} {onchange} />
</div>
