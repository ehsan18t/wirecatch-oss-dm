<script lang="ts">
  // Speed and queues: a limit on all downloads together, how many run at once across the queues, and how many connections each opens, in the ranges the core accepts, each with the queues that use their own value; then every queue, with when it runs, its own limits and its rules, Edit opening its dialog and New queue in the name line, and Test a link on a row of its own.
  import Pencil from '~icons/lucide/pencil';
  import Plus from '~icons/lucide/plus';
  import {
    MAX_CONCURRENT_DOWNLOADS,
    MAX_CONNECTIONS_PER_DOWNLOAD,
    type Queue,
  } from '@wirecatch/protocol';
  import { FIRST_TOTAL_SPEED_LIMIT, inUnit, SPEED_FIELD, toBytes } from '@wirecatch/units';
  import type { Snippet } from 'svelte';
  import { Badge, Button, IconButton, NumberInput } from '@wirecatch/ui';
  import { formatSchedule } from '$lib/format';
  import { queueLimitFacts, queuesWithOwn } from '$lib/limits';
  import { rulesFact } from '$lib/queue-rules';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import TryLink from '../TryLink.svelte';
  import ItemList from './ItemList.svelte';
  import OwnValueQueues from './OwnValueQueues.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';
  import SettingToggle from './SettingToggle.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const app = getApp();
  const notes: Partial<Record<SettingIn<'speed'>, Snippet>> = {
    global_speed_limit: ownSpeed,
    max_concurrent_downloads: ownConcurrent,
    connections_per_download: ownConnections,
  };

  const controls: Record<SettingIn<'speed'>, Snippet<[Row]>> = {
    global_speed_limit: speedLimit,
    max_concurrent_downloads: concurrent,
    connections_per_download: connections,
    queues,
    try_link: tryLink,
  };
</script>

<SettingRows
  {items}
  {settings}
  {flash}
  {controls}
  {notes}
  stacked={['queues', 'try_link']}
  unlabelled={['queues', 'try_link']}
  actions={{ queues: newQueue }}
/>

{#snippet newQueue()}
  <Button size="sm" icon={Plus} onclick={() => app.openDialog({ kind: 'queue', id: null })}
    >New queue</Button
  >
{/snippet}

{#snippet tryLink()}<TryLink inRow />{/snippet}

{#snippet ownSpeed()}<OwnValueQueues
    queues={queuesWithOwn(app.queues.list, 'speed_limit')}
  />{/snippet}
{#snippet ownConcurrent()}<OwnValueQueues
    queues={queuesWithOwn(app.queues.list, 'max_concurrent')}
  />{/snippet}
{#snippet ownConnections()}<OwnValueQueues
    queues={queuesWithOwn(app.queues.list, 'connections')}
  />{/snippet}

{#snippet speedLimit({ def, s, control }: Row)}
  {#if s.global_speed_limit !== null}
    <NumberInput
      label="Global speed limit"
      class="w-32"
      unit="MB/s"
      min={SPEED_FIELD.MB.min}
      step={SPEED_FIELD.MB.step}
      value={inUnit(s.global_speed_limit, 'MB')}
      onchange={(value: number | null) => {
        const bytes = value === null ? null : toBytes(value, 'MB');
        if (bytes !== null && bytes > 0) save((d) => void (d.global_speed_limit = bytes));
      }}
    />
  {/if}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.global_speed_limit !== null}
    onchange={(v: boolean) =>
      save((d) => void (d.global_speed_limit = v ? FIRST_TOTAL_SPEED_LIMIT : null))}
  />
{/snippet}

{#snippet concurrent({ s, control }: Row)}
  <NumberInput
    {...control}
    class="w-28"
    min={1}
    max={MAX_CONCURRENT_DOWNLOADS}
    value={s.max_concurrent_downloads}
    onchange={(value: number | null) => {
      if (value !== null) save((d) => void (d.max_concurrent_downloads = value));
    }}
  />
{/snippet}

{#snippet connections({ s, control }: Row)}
  <NumberInput
    {...control}
    class="w-28"
    min={1}
    max={MAX_CONNECTIONS_PER_DOWNLOAD}
    value={s.connections_per_download}
    onchange={(value: number | null) => {
      if (value !== null) save((d) => void (d.connections_per_download = value));
    }}
  />
{/snippet}

{#snippet queues({ control }: Row)}
  <ItemList
    items={app.queues.list}
    key={(queue: Queue) => queue.id}
    label="Your queues"
    empty="No queues."
    describedby={control['aria-describedby']}
  >
    {#snippet row(queue: Queue)}
      {const facts = $derived([
        queue.status.paused ? 'Paused' : formatSchedule(queue.settings.schedule),
        ...(queue.settings.notify === 'when_done' ? ['Notifies you when the queue is done'] : []),
        ...queueLimitFacts(queue.settings),
        ...[rulesFact(queue.settings.add_downloads)].filter((fact) => fact !== null),
      ])}
      <span class="flex min-w-0 flex-1 flex-col gap-0.5">
        <span class="flex items-center gap-2 text-ui font-medium text-ink">
          <span class="truncate">{queue.settings.name}</span>
          {#if queue.is_default}<Badge size="sm">Default</Badge>{/if}
        </span>
        {#if queue.settings.description}
          <span class="truncate text-small text-ink">{queue.settings.description}</span>
        {/if}
        <span class="truncate text-small text-ink-muted">{facts.join(' · ')}</span>
      </span>
      <IconButton
        icon={Pencil}
        label="Edit {queue.settings.name}"
        onclick={() => app.openDialog({ kind: 'queue', id: queue.id })}
      />
    {/snippet}
  </ItemList>
{/snippet}
