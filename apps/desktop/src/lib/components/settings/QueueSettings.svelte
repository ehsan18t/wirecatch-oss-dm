<script lang="ts">
  // Queues: every queue in one table, with when it runs, how many of its downloads run at once and its speed limit, so ten queues stay manageable. Edit opens the queue's own dialog; New queue makes one.
  import Pencil from '~icons/lucide/pencil';
  import Plus from '~icons/lucide/plus';
  import type { Snippet } from 'svelte';
  import { Button } from '@wirecatch/ui';
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

<SettingRows {items} {settings} {flash} {controls} />

{#snippet queues({ helpId }: Row)}
  <div class="flex w-full flex-col items-start gap-3">
    <table class="w-full border-collapse text-small" aria-describedby={helpId}>
      <thead>
        <tr class="text-left text-caption font-semibold tracking-caps text-ink-subtle uppercase">
          <th class="py-1.5 pr-3 font-semibold">Queue</th>
          <th class="py-1.5 pr-3 font-semibold">Runs</th>
          <th class="py-1.5 pr-3 text-right font-semibold">At once</th>
          <th class="py-1.5 pr-3 text-right font-semibold">Limit</th>
          <th><span class="sr-only">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        {#each app.queues.list as queue (queue.id)}
          <tr class="border-t border-line">
            <th scope="row" class="py-1.5 pr-3 text-left font-medium text-ink">
              {queue.name}{#if queue.is_default}<span class="ml-2 font-normal text-ink-subtle"
                  >default</span
                >{/if}
            </th>
            <td class="py-1.5 pr-3 text-ink-muted"
              >{queue.paused ? 'Paused' : formatSchedule(queue.schedule)}</td
            >
            <td class="py-1.5 pr-3 text-right text-ink-muted tabular-nums"
              >{queue.max_concurrent}</td
            >
            <td class="py-1.5 pr-3 text-right text-ink-muted tabular-nums"
              >{queue.speed_limit === null ? 'None' : formatRate(queue.speed_limit)}</td
            >
            <td class="py-1 text-right">
              <Button
                size="sm"
                variant="ghost"
                icon={Pencil}
                aria-label="Edit {queue.name}"
                onclick={() => app.openDialog({ kind: 'queue', id: queue.id })}>Edit</Button
              >
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
    <Button size="sm" icon={Plus} onclick={() => app.openDialog({ kind: 'queue', id: null })}
      >New queue</Button
    >
  </div>
{/snippet}
