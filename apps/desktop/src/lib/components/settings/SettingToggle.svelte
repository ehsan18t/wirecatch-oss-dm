<script lang="ts">
  // A setting's on/off switch: the design system's, labelled and described by its row.
  import { Switch, type FieldControl } from '@wirecatch/ui';

  let {
    control,
    label,
    checked,
    disabled = false,
    onchange,
  }: {
    control: FieldControl;
    label: string;
    checked: boolean;
    disabled?: boolean;
    onchange: (value: boolean) => void;
  } = $props();
</script>

<Switch
  id={control.id}
  describedby={control['aria-describedby']}
  {label}
  hideLabel
  {checked}
  {disabled}
  {onchange}
/>
