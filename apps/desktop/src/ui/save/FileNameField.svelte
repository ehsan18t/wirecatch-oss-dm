<script lang="ts">
  // The name a download is saved under. The placeholder shows the name it gets when left empty. The Add dialog uses it, and the download window's save question can.
  import { Field, type FieldControl, Input } from '@wirecatch/ui';

  interface Props {
    value: string;
    onchange: (value: string) => void;
    /** The name the file gets when the field is left empty. */
    placeholder?: string | undefined;
    description?: string | undefined;
    disabled?: boolean;
    class?: string | undefined;
  }

  let {
    value,
    onchange,
    placeholder,
    description,
    disabled = false,
    class: className,
  }: Props = $props();
</script>

<Field label="File name" {description} class={className}>
  {#snippet children(control: FieldControl)}
    <Input
      {...control}
      {value}
      {onchange}
      {placeholder}
      {disabled}
      spellcheck="false"
      autocomplete="off"
    />
  {/snippet}
</Field>
