<script lang="ts">
  // Fields whose owner removes them while one has focus, as a dialog closing on Escape does. Chromium sends the focused field a blur as it leaves the page, after its component is gone.
  import Field from '../primitives/Field.svelte';
  import Input from '../primitives/Input.svelte';
  import TextArea from '../primitives/TextArea.svelte';
  import type { FieldControl } from '../primitives/field-control';

  let shown = $state(true);
  // What the fields say follows the owner's state, which the removal changes.
  const hint = $derived(shown ? 'As it is saved.' : '');
</script>

<button type="button" onclick={() => (shown = false)}>Done</button>
{#if shown}
  <Field label="Name" description={hint}>
    {#snippet children(control: FieldControl)}
      <Input {...control} value="a.zip" />
    {/snippet}
  </Field>
  <Field label="Links" description={hint}>
    {#snippet children(control: FieldControl)}
      <TextArea {...control} value="https://a.example/a.zip" />
    {/snippet}
  </Field>
{/if}
