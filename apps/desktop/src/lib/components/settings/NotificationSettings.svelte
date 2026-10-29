<script lang="ts">
  // Notifications: how Wirecatch tells you a download started (a note in the browser, D65), finished or failed (D52), and whether notifications show during the system's Do Not Disturb (D64). When it tells about finished downloads is each queue's choice, so the default queue's is named under the row, with a way to change it; a notice time that does not apply says why.
  import type {
    NotifyOnFinish,
    FinishNoticeTime,
    NotifyOnStart,
    StartNoticeTime,
  } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { Button, Select } from '@wirecatch/ui';
  import {
    FINISH_NOTICE_TIME_OPTIONS,
    NOTIFY_ON_FINISH_OPTIONS,
    queueNotifyLine,
    START_NOTICE_TIME_OPTIONS,
    NOTIFY_ON_START_OPTIONS,
  } from '$lib/notify-options';
  import { getApp } from '$lib/state/app.svelte';
  import type { SettingIn } from '$lib/settings-schema';
  import type { Row, SectionProps } from './row';
  import RowNote from './RowNote.svelte';
  import SettingRows from './SettingRows.svelte';
  import SettingToggle from './SettingToggle.svelte';

  let { items, settings, flash, save }: SectionProps = $props();
  const app = getApp();

  const controls: Record<SettingIn<'notifications'>, Snippet<[Row]>> = {
    notify_on_start: notifyOnStart,
    start_notice_time: startNoticeTime,
    notify_on_finish: notifyOnFinish,
    finish_notice_time: finishNoticeTime,
    notify_during_do_not_disturb: notifyDuringDoNotDisturb,
  };

  const notes: Partial<Record<SettingIn<'notifications'>, Snippet>> = {
    start_notice_time: startTimeNote,
    notify_on_finish: finishQueueNote,
    finish_notice_time: finishTimeNote,
  };

  const defaultQueue = $derived(app.queues.defaultQueue);
</script>

<SettingRows {items} {settings} {flash} {controls} {notes} />

{#snippet startTimeNote()}
  {#if settings.notify_on_start !== 'browser'}
    <RowNote>Used only with “Show a notice in the browser”.</RowNote>
  {/if}
{/snippet}

{#snippet finishTimeNote()}
  {#if settings.notify_on_finish !== 'browser'}
    <RowNote>Used only with “Show a notice in the browser”.</RowNote>
  {/if}
{/snippet}

{#snippet finishQueueNote()}
  {#if defaultQueue && settings.notify_on_finish !== 'nothing'}
    <RowNote>
      {queueNotifyLine(defaultQueue.settings.name, defaultQueue.settings.notify)}
      <Button
        variant="link"
        size="sm"
        aria-label="Change when {defaultQueue.settings.name} notifies you"
        onclick={() => app.openDialog({ kind: 'queue', id: defaultQueue.id })}>Change</Button
      >
    </RowNote>
  {/if}
{/snippet}

{#snippet notifyOnStart({ s, control }: Row)}
  <Select
    {...control}
    class="w-56"
    value={s.notify_on_start}
    options={NOTIFY_ON_START_OPTIONS}
    onchange={(notice: NotifyOnStart) => save((d) => void (d.notify_on_start = notice))}
  />
{/snippet}

{#snippet startNoticeTime({ s, control }: Row)}
  <Select
    {...control}
    class="w-56"
    value={s.start_notice_time}
    options={START_NOTICE_TIME_OPTIONS}
    disabled={s.notify_on_start !== 'browser'}
    onchange={(time: StartNoticeTime) => save((d) => void (d.start_notice_time = time))}
  />
{/snippet}

{#snippet notifyOnFinish({ s, control }: Row)}
  <Select
    {...control}
    class="w-56"
    value={s.notify_on_finish}
    options={NOTIFY_ON_FINISH_OPTIONS}
    onchange={(notice: NotifyOnFinish) => save((d) => void (d.notify_on_finish = notice))}
  />
{/snippet}

{#snippet finishNoticeTime({ s, control }: Row)}
  <Select
    {...control}
    class="w-56"
    value={s.finish_notice_time}
    options={FINISH_NOTICE_TIME_OPTIONS}
    disabled={s.notify_on_finish !== 'browser'}
    onchange={(time: FinishNoticeTime) => save((d) => void (d.finish_notice_time = time))}
  />
{/snippet}

{#snippet notifyDuringDoNotDisturb({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.notify_during_do_not_disturb}
    onchange={(v: boolean) => save((d) => void (d.notify_during_do_not_disturb = v))}
  />
{/snippet}
