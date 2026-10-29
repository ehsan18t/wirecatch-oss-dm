<script lang="ts">
  // Notifications: how Wirecatch tells you a download started (a note in the browser, D65), finished or failed (D52), and whether notifications show during the system's Do Not Disturb (D64). When it tells about finished downloads is each queue's choice, so the default queue's is named under the dropdown, with a way to change it.
  import type {
    FinishNotice,
    FinishNoticeTime,
    StartNotice,
    StartNoticeTime,
  } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { Button, Select } from '@wirecatch/ui';
  import {
    FINISH_NOTICE_TIME_OPTIONS,
    FINISH_NOTICE_OPTIONS,
    queueNotifyLine,
    START_NOTICE_TIME_OPTIONS,
    START_NOTICE_OPTIONS,
  } from '$lib/finish-notices';
  import { getApp } from '$lib/state/app.svelte';
  import type { SettingIn } from '$lib/settings-schema';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';
  import SettingToggle from './SettingToggle.svelte';

  let { items, settings, flash, save }: SectionProps = $props();
  const app = getApp();

  const controls: Record<SettingIn<'notifications'>, Snippet<[Row]>> = {
    start_notice: startNotice,
    start_notice_time: startNoticeTime,
    finish_notice: finishNotice,
    finish_notice_time: finishNoticeTime,
    notify_during_do_not_disturb: notifyDuringDoNotDisturb,
  };

  const defaultQueue = $derived(app.queues.defaultQueue);
</script>

<SettingRows {items} {settings} {flash} {controls} stacked={['finish_notice']} />

{#snippet startNotice({ s, control }: Row)}
  <Select
    {...control}
    class="w-56"
    value={s.start_notice}
    options={START_NOTICE_OPTIONS}
    onchange={(notice: StartNotice) => save((d) => void (d.start_notice = notice))}
  />
{/snippet}

{#snippet startNoticeTime({ s, control }: Row)}
  <Select
    {...control}
    class="w-56"
    value={s.start_notice_time}
    options={START_NOTICE_TIME_OPTIONS}
    disabled={s.start_notice !== 'browser'}
    onchange={(time: StartNoticeTime) => save((d) => void (d.start_notice_time = time))}
  />
{/snippet}

{#snippet finishNotice({ s, control }: Row)}
  <div class="flex flex-col items-start gap-1.5">
    <Select
      {...control}
      class="w-56"
      value={s.finish_notice}
      options={FINISH_NOTICE_OPTIONS}
      onchange={(notice: FinishNotice) => save((d) => void (d.finish_notice = notice))}
    />
    {#if defaultQueue && s.finish_notice !== 'nothing'}
      <p class="flex flex-wrap items-baseline gap-x-1.5 text-small text-ink-muted">
        <span>{queueNotifyLine(defaultQueue.settings.name, defaultQueue.settings.notify)}</span>
        <Button
          variant="link"
          size="sm"
          aria-label="Change when {defaultQueue.settings.name} notifies you"
          onclick={() => app.openDialog({ kind: 'queue', id: defaultQueue.id })}>Change</Button
        >
      </p>
    {/if}
  </div>
{/snippet}

{#snippet finishNoticeTime({ s, control }: Row)}
  <Select
    {...control}
    class="w-56"
    value={s.finish_notice_time}
    options={FINISH_NOTICE_TIME_OPTIONS}
    disabled={s.finish_notice !== 'browser'}
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
