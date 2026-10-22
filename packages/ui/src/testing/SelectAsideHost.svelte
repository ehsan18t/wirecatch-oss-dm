<script lang="ts">
  import Select from '../primitives/Select.svelte';
  import type { SelectOption } from '../primitives/select';

  let { onchange }: { onchange: (value: string) => void } = $props();

  const options: SelectOption[] = [
    { value: '2160', label: '2160p' },
    { value: '1080', label: '1080p' },
    { value: '720', label: '720p' },
  ];
  const sizes: Record<string, string> = { '2160': '1.80 GB', '1080': '612 MB', '720': '298 MB' };
</script>

<Select value="2160" {options} label="Quality" searchable {onchange}>
  {#snippet aside(option: SelectOption)}
    <span>{sizes[option.value]}</span>
  {/snippet}
</Select>
