<script lang="ts" module>
  import type { SelectOption } from '@wirecatch/ui';
  import type { DayPattern } from '$lib/format';

  const UNITS: readonly SelectOption<'KB' | 'MB'>[] = [
    { value: 'KB', label: 'KB/s' },
    { value: 'MB', label: 'MB/s' },
  ];

  const PATTERNS: readonly SelectOption<DayPattern>[] = [
    { value: 'every', label: 'Every day' },
    { value: 'weekdays', label: 'Weekdays' },
    { value: 'weekends', label: 'Weekends' },
    { value: 'custom', label: 'Pick days' },
  ];
</script>

<script lang="ts">
  // A queue's name, how many of its downloads run at once, its speed limit, its hours and when it tells you its downloads finished: each limit is a switch that opens its own fields, and the days are a pattern first (every day, weekdays, weekends) with single days only when asked for.
  import {
    MAX_CONCURRENT_DOWNLOADS,
    NEW_QUEUE_MAX_CONCURRENT,
    type Queue,
    type QueueId,
    type QueueNotify,
    type TimeOfDay,
    type Weekday,
  } from '@wirecatch/protocol';
  import { MB, splitKbOrMb, toBytes } from '@wirecatch/units';
  import {
    Button,
    Callout,
    Checkbox,
    Dialog,
    Field,
    Input,
    NumberInput,
    SegmentedControl,
    Switch,
    ToggleChip,
    type FieldControl,
  } from '@wirecatch/ui';
  import { QUEUE_NOTIFY_OPTIONS } from '$lib/finish-notices';
  import { dayPattern, daysInPhrase, formatTime, parseTime, WEEKDAY_SHORT } from '$lib/format';
  import { DAYS_PER_WEEK, WEEKDAYS, WEEKEND, WORKDAYS } from '$lib/time';
  import { queueConcurrentProblem, SPEED_FIELD } from '$lib/limits';
  import { getApp } from '$lib/state/app.svelte';
  import type { QueueChanges } from '$lib/state/queues.svelte';
  import TimeInput from './TimeInput.svelte';

  let { id, deleting = false }: { id: QueueId | null; deleting?: boolean } = $props();
  const app = getApp();

  // A snapshot of the queue when the dialog opened; the form edits its own copy.
  const original = (() => (id === null ? undefined : app.queues.byId(id)))();
  // What the form suggests when a limit or schedule is turned on for a queue that has none.
  // The core makes a new queue without either; these are only where the fields start.
  const FIRST_SPEED_LIMIT = 2 * MB;
  const FIRST_START: TimeOfDay = { hour: 1, minute: 0 };
  const FIRST_STOP: TimeOfDay = { hour: 7, minute: 0 };
  const formId = $props.id();

  let name = $state(original?.name ?? '');
  let maxConcurrent = $state(original?.max_concurrent ?? NEW_QUEUE_MAX_CONCURRENT);
  let limitOn = $state(original !== undefined && original.speed_limit !== null);
  const initialLimit = splitKbOrMb(original?.speed_limit ?? FIRST_SPEED_LIMIT);
  let limitUnit = $state(initialLimit.unit);
  let limitValue = $state<number | null>(initialLimit.value);
  let scheduleOn = $state(original !== undefined && original.schedule !== null);
  let start = $state(formatTime(original?.schedule?.start ?? FIRST_START));
  let untilEmpty = $state(original?.schedule ? original.schedule.stop === null : false);
  let stop = $state(formatTime(original?.schedule?.stop ?? FIRST_STOP));
  let days = $state<Weekday[]>(original?.schedule?.days ?? []);
  // Picking single days is its own choice, kept while the days picked happen to make a pattern.
  let pickingDays = $state(dayPattern(original?.schedule?.days ?? []) === 'custom');
  let notify = $state<QueueNotify>(original?.notify ?? 'each_download');

  /** What the queue's choice of when to tell does, given how Settings says to tell. */
  const notifyHelp = $derived.by(() => {
    if (app.settings?.finish_notice === 'nothing')
      return 'Settings, General, says to tell you nothing, so this applies once you change that.';
    return notify === 'each_download'
      ? 'Downloads that finish a few seconds apart are grouped together.'
      : 'Tells you once, when nothing in it downloads or waits to start. You still hear about a failed download right away.';
  });
  let error = $state('');
  // Opened to delete the queue (from the palette), it starts at that question.
  let confirmDelete = $state((() => deleting)());
  let busy = $state(false);
  /** A queue this dialog already created, so a retried save updates it instead of making another. */
  let created = $state<Queue>();

  // The button pressed goes as the footer changes: the focus moves to the safe answer, and back to Delete queue on Keep it.
  let keepButton = $state<HTMLButtonElement | null>(null);
  let deleteButton = $state<HTMLButtonElement | null>(null);
  let asked = (() => confirmDelete)();
  $effect(() => {
    // Waits for the new button to be there: it binds after the footer changes.
    const target = confirmDelete ? keepButton : deleteButton;
    if (confirmDelete === asked || !target) return;
    asked = confirmDelete;
    target.focus();
  });

  /** What deleting the queue does with its downloads. */
  const movingLine = $derived.by(() => {
    if (!original) return '';
    const count = app.downloads.idsInQueue(original.id).length;
    const to = app.queues.defaultQueue?.name ?? 'the default queue';
    if (count === 0) return 'It holds no downloads.';
    return count === 1
      ? `Its download moves to ${to}.`
      : `Its ${String(count)} downloads move to ${to}.`;
  });

  const pattern = $derived<DayPattern>(pickingDays ? 'custom' : dayPattern(days));
  const DAYS_OF: Record<Exclude<DayPattern, 'custom'>, readonly Weekday[]> = {
    every: [],
    weekdays: WORKDAYS,
    weekends: WEEKEND,
  };

  function choosePattern(next: DayPattern) {
    pickingDays = next === 'custom';
    // Picking days starts from the days it runs on now: every day is all seven ticked.
    if (next === 'custom') days = days.length === 0 ? [...WEEKDAYS] : days;
    else days = [...DAYS_OF[next]];
  }

  // Picking days limits the queue to those days; picking none means every day.
  function toggleDay(d: Weekday) {
    days = days.includes(d) ? days.filter((x) => x !== d) : [...days, d];
  }

  const daysHelp = $derived.by(() => {
    if (days.length === 0) return 'Pick at least one day.';
    if (days.length === DAYS_PER_WEEK) return 'Runs every day.';
    return `Runs only on ${daysInPhrase(days)}.`;
  });

  /** The speed limit the form describes, or why it is not one. */
  function readLimit(): { limit: Queue['speed_limit'] } | string {
    if (!limitOn) return { limit: null };
    const limit = limitValue === null ? null : toBytes(limitValue, limitUnit);
    return limit === null || limit <= 0
      ? 'Enter a speed limit above zero, or turn the limit off.'
      : { limit };
  }

  /** The schedule the form describes, or why it is not one. */
  function readSchedule(): { schedule: Queue['schedule'] } | string {
    if (!scheduleOn) return { schedule: null };
    const s = parseTime(start);
    const e = untilEmpty ? null : parseTime(stop);
    if (!s) return `Enter a start time like ${formatTime(FIRST_START)}.`;
    if (!untilEmpty && !e) return `Enter a stop time like ${formatTime(FIRST_STOP)}.`;
    if (e?.hour === s.hour && e.minute === s.minute)
      return 'The start and stop times are the same. Pick a different stop time.';
    if (pickingDays && days.length === 0) return 'Pick at least one day for the queue to run.';
    // Seven ticked days is the same as every day.
    const every = days.length === DAYS_PER_WEEK;
    return {
      schedule: { start: s, stop: e, days: every ? [] : WEEKDAYS.filter((d) => days.includes(d)) },
    };
  }

  /** What the form changes in a queue, or why it cannot. */
  function readForm(): QueueChanges | string {
    if (!name.trim()) return 'Give the queue a name.';
    const tooMany = queueConcurrentProblem(maxConcurrent);
    if (tooMany !== null) return tooMany;
    const limit = readLimit();
    if (typeof limit === 'string') return limit;
    const schedule = readSchedule();
    if (typeof schedule === 'string') return schedule;
    return {
      name: name.trim(),
      max_concurrent: Math.round(maxConcurrent),
      speed_limit: limit.limit,
      schedule: schedule.schedule,
      notify,
    };
  }

  async function save() {
    if (busy) return;
    error = '';
    busy = true;
    try {
      const changes = readForm();
      if (typeof changes === 'string') {
        error = changes;
        return;
      }
      const saved = await app.queues.save(original ?? created, changes);
      if (saved.kind === 'made') created = saved.queue;
      if (saved.kind === 'saved') {
        app.closeDialog();
        if (!original) app.show({ kind: 'queue', id: saved.queue.id });
      } else if (created) {
        error = 'The queue was made, but its settings were not saved. Save again to retry.';
      }
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (original && (await app.queues.remove(original.id))) app.closeDialog();
  }
</script>

<Dialog
  open
  title={original ? `Edit ${original.name}` : 'New queue'}
  description={original
    ? undefined
    : 'A queue runs its downloads in order, with its own limits and hours.'}
  onclose={() => app.closeDialog()}
>
  <form
    id={formId}
    class="flex flex-col gap-5 pb-1"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    <div class="flex flex-wrap items-start gap-4">
      <Field label="Name" class="flex-1 basis-56">
        {#snippet children(control: FieldControl)}
          <Input
            {...control}
            value={name}
            onchange={(value: string) => (name = value)}
            placeholder="For example: Overnight"
            required
          />
        {/snippet}
      </Field>
      <Field label="Downloads at the same time" description="The rest wait their turn.">
        {#snippet children(control: FieldControl)}
          <NumberInput
            {...control}
            class="w-28"
            value={maxConcurrent}
            min={1}
            max={MAX_CONCURRENT_DOWNLOADS}
            onchange={(value: number | null) => {
              if (value !== null) maxConcurrent = value;
            }}
          />
        {/snippet}
      </Field>
    </div>

    <section class="flex flex-col gap-3 rounded-md border border-line bg-layer p-3">
      <Switch
        labelSide="start"
        label="Limit this queue's speed"
        description="Shared by everything running in this queue."
        checked={limitOn}
        onchange={(on: boolean) => (limitOn = on)}
      />
      {#if limitOn}
        <div class="flex items-center gap-2">
          <NumberInput
            label="Speed limit"
            class="w-28"
            value={limitValue}
            min={SPEED_FIELD[limitUnit].min}
            step={SPEED_FIELD[limitUnit].step}
            onchange={(value: number | null) => (limitValue = value)}
          />
          <SegmentedControl
            label="Unit"
            size="sm"
            value={limitUnit}
            options={UNITS}
            onchange={(unit: 'KB' | 'MB') => (limitUnit = unit)}
          />
        </div>
      {/if}
    </section>

    <section class="flex flex-col gap-3 rounded-md border border-line bg-layer p-3">
      <Switch
        labelSide="start"
        label="Only run at certain times"
        description="Outside these hours its downloads wait, unless you start the queue now."
        checked={scheduleOn}
        onchange={(on: boolean) => (scheduleOn = on)}
      />
      {#if scheduleOn}
        <div class="flex flex-wrap items-end gap-x-4 gap-y-3">
          <Field label="Starts at">
            {#snippet children(control: FieldControl)}
              <TimeInput {...control} value={start} onchange={(value: string) => (start = value)} />
            {/snippet}
          </Field>
          {#if !untilEmpty}
            <Field label="Stops at">
              {#snippet children(control: FieldControl)}
                <TimeInput {...control} value={stop} onchange={(value: string) => (stop = value)} />
              {/snippet}
            </Field>
          {/if}
          <Checkbox
            class="pb-2"
            label="Run until the queue is empty"
            checked={untilEmpty}
            onchange={(on: boolean) => (untilEmpty = on)}
          />
        </div>
        <div class="flex flex-col gap-2">
          <span class="text-ui font-medium text-ink" aria-hidden="true">Days</span>
          <SegmentedControl
            label="Days"
            size="sm"
            value={pattern}
            options={PATTERNS}
            onchange={choosePattern}
          />
          {#if pattern === 'custom'}
            <div
              class="flex flex-wrap gap-1"
              role="group"
              aria-label="Only on these days"
              aria-describedby="{formId}-days-help"
            >
              {#each WEEKDAYS as d (d)}
                <ToggleChip
                  pressed={days.includes(d)}
                  label={WEEKDAY_SHORT[d]}
                  onchange={() => toggleDay(d)}
                />
              {/each}
            </div>
            <p id="{formId}-days-help" class="text-small text-ink-muted">{daysHelp}</p>
          {/if}
        </div>
      {/if}
    </section>

    <section
      class="flex flex-wrap items-center justify-between gap-3 rounded-md border border-line bg-layer p-3"
      aria-labelledby="{formId}-notify"
      aria-describedby="{formId}-notify-help"
    >
      <div class="flex min-w-0 flex-1 basis-56 flex-col gap-0.5">
        <span id="{formId}-notify" class="text-ui font-medium text-ink"
          >Tell me when downloads finish</span
        >
        <span id="{formId}-notify-help" class="text-small text-ink-muted">{notifyHelp}</span>
      </div>
      <SegmentedControl
        label="Tell me when downloads finish"
        size="sm"
        value={notify}
        options={QUEUE_NOTIFY_OPTIONS}
        onchange={(next: QueueNotify) => (notify = next)}
      />
    </section>

    {#if error}
      <Callout tone="danger" live="assertive">{error}</Callout>
    {/if}
  </form>

  {#snippet footer()}
    {#if confirmDelete && original}
      <p class="mr-auto text-small text-ink-muted" role="status">
        {movingLine}
      </p>
      <Button bind:ref={keepButton} data-autofocus onclick={() => (confirmDelete = false)}
        >Keep it</Button
      >
      <Button variant="danger" onclick={remove}>Delete {original.name}</Button>
    {:else}
      {#if original && !original.is_default}
        <Button
          bind:ref={deleteButton}
          variant="danger-quiet"
          class="mr-auto"
          onclick={() => (confirmDelete = true)}>Delete queue</Button
        >
      {/if}
      <Button onclick={() => app.closeDialog()}>Cancel</Button>
      <Button type="submit" form={formId} variant="primary" loading={busy}>
        {original || created ? 'Save' : 'Create queue'}
      </Button>
    {/if}
  {/snippet}
</Dialog>
