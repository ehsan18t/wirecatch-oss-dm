<script lang="ts">
  // Queues: every queue in one list, with when it runs, how many of its downloads run at once, its speed limit and whether it tells you only once it is done, so ten queues stay manageable. Edit opens the queue's own dialog; New queue makes one.
  import Pencil from '~icons/lucide/pencil';
  import Plus from '~icons/lucide/plus';
  import type { Snippet } from 'svelte';
  import { Badge, Button } from '@wirecatch/ui';
  import { formatRate } from '@wirecatch/units';
  import { formatSchedule } from '$lib/format';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash }: SectionProps = $props();

  const app = getApp();

  const controls: Record<SettingIn<'queues'>, Snippet<[Row]>> = { queues };
</script>

<SettingRows {items} {settings} {flash} {controls} stacked={['queues']} unlabelled={['queues']} />

{#snippet queues({ control }: Row)}
  <div class="flex w-full flex-col items-start gap-3">
    <ul
      aria-label="Your queues"
      aria-describedby={control['aria-describedby']}
      class="flex w-full flex-col divide-y divide-line rounded-md border border-line"
    >
      {#each app.queues.list as queue (queue.id)}
        {const facts = $derived([
          queue.paused ? 'Paused' : formatSchedule(queue.schedule),
          `${String(queue.max_concurrent)} at once`,
          queue.speed_limit === null ? 'No speed limit' : `Up to ${formatRate(queue.speed_limit)}`,
          ...(queue.notify === 'when_done' ? ['Tells you once the queue is done'] : []),
        ])}
        <li class="flex items-center gap-3 py-2 pr-2 pl-3">
          <span class="flex min-w-0 flex-1 flex-col gap-0.5">
            <span class="flex items-center gap-2 text-ui font-medium text-ink">
              <span class="truncate">{queue.name}</span>
              {#if queue.is_default}<Badge size="sm">Default</Badge>{/if}
            </span>
            <span class="truncate text-small text-ink-muted">{facts.join(' · ')}</span>
          </span>
          <Button
            size="sm"
            variant="ghost"
            icon={Pencil}
            aria-label="Edit {queue.name}"
            onclick={() => app.openDialog({ kind: 'queue', id: queue.id })}>Edit</Button
          >
        </li>
      {/each}
    </ul>
    <Button size="sm" icon={Plus} onclick={() => app.openDialog({ kind: 'queue', id: null })}
      >New queue</Button
    >
  </div>
{/snippet}
