<script lang="ts">
  import Input from '../primitives/Input.svelte';

  // An owner that rewrites what is typed: a file name with no spaces.
  let value = $state('');
</script>

<Input label="File name" {value} onchange={(next: string) => (value = next.replaceAll(' ', ''))} />
