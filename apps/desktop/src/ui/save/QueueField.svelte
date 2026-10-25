<script lang="ts">
  // Which queue a download joins, the default one marked in the list (not in the chosen value, where it would be noise). The Add dialog uses it, and the download window's save question can.
  import type { Queue, QueueId } from '@wirecatch/protocol';
  import { Field, type FieldControl, Select } from '@wirecatch/ui';

  interface Props {
    queues: readonly Queue[];
    /** Null while no queue is listed yet: the core then uses its default. */
    value: QueueId | null;
    onchange: (queue: QueueId) => void;
    description?: string | undefined;
    class?: string | undefined;
  }

  let { queues, value, onchange, description, class: className }: Props = $props();

  const options = $derived(
    queues.map((q) => ({
      value: q.id,
      label: q.settings.name,
      description: q.is_default ? 'The default queue' : undefined,
    })),
  );
</script>

<Field label="Queue" {description} class={className}>
  {#snippet children(control: FieldControl)}
    <Select {...control} {value} {options} placeholder="The default queue" {onchange} />
  {/snippet}
</Field>
