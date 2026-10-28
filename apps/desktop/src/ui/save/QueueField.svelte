<script lang="ts">
  // Which queue a download joins, the default one marked in the list (not in the chosen value, where it would be noise). With `automatic`, Automatic comes first and stands for `null`: each link goes where the claims send it (D72.8). The Add dialog and the download windows' questions use it.
  import type { Queue, QueueId } from '@wirecatch/protocol';
  import { Field, type FieldControl, Select, type SelectOption } from '@wirecatch/ui';
  import { claimsFact } from '$lib/queue-claims';

  interface Props {
    queues: readonly Queue[];
    /** Null for Automatic, or while no queue is listed yet: the core then decides. */
    value: QueueId | null;
    onchange: (queue: QueueId | null) => void;
    automatic?: boolean;
    description?: string | undefined;
    class?: string | undefined;
  }

  let {
    queues,
    value,
    onchange,
    automatic = false,
    description,
    class: className,
  }: Props = $props();

  const AUTOMATIC = 'automatic';
  type Choice = QueueId | typeof AUTOMATIC;
  const AUTOMATIC_OPTION: SelectOption<Choice> = {
    value: AUTOMATIC,
    label: 'Automatic',
    description: 'To the queue that takes it',
  };

  const options = $derived<SelectOption<Choice>[]>([
    ...(automatic ? [AUTOMATIC_OPTION] : []),
    ...queues.map((q) => ({
      value: q.id,
      label: q.settings.name,
      description:
        [q.is_default ? 'The default queue' : null, claimsFact(q.settings.takes)]
          .filter((fact) => fact !== null)
          .join(' · ') || undefined,
    })),
  ]);
</script>

<Field label="Queue" {description} class={className}>
  {#snippet children(control: FieldControl)}
    <Select
      {...control}
      value={value ?? (automatic ? AUTOMATIC : null)}
      {options}
      placeholder="The default queue"
      onchange={(picked: Choice) => onchange(picked === AUTOMATIC ? null : picked)}
    />
  {/snippet}
</Field>
