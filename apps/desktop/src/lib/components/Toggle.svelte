<script lang="ts">
  // A setting's on/off switch: the design system's, named by the setting and described by its help line in the row.
  import { Switch } from '@wirecatch/ui';

  let {
    checked,
    label,
    describedby,
    onchange,
  }: {
    checked: boolean;
    label: string;
    describedby?: string;
    onchange: (value: boolean) => void;
  } = $props();
</script>

<Switch {checked} {label} hideLabel {describedby} {onchange} />
