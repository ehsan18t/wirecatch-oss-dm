<script lang="ts">
  import Field from '../primitives/Field.svelte';
  import type { FieldControl } from '../primitives/field-control';
  import Input from '../primitives/Input.svelte';

  let {
    error = null,
    help,
    hideLabel = false,
  }: { error?: string | null; help?: string; hideLabel?: boolean } = $props();
  let value = $state('https://');
</script>

<Field label="Address" description="A link to a file." {error} {help} {hideLabel}>
  {#snippet children(control: FieldControl)}
    <Input {...control} {value} onchange={(next: string) => (value = next)} />
  {/snippet}
</Field>
<p data-testid="value">{value}</p>
