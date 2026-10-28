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
  // A queue's name and description, its own limits, its hours and when it tells you its downloads finished: each limit is a switch that opens its own fields, and off follows the app (D72.1); the days are a pattern first (every day, weekdays, weekends) with single days only when asked for.
  import {
    DEFAULT_CONNECTIONS_PER_DOWNLOAD,
    DEFAULT_MAX_CONCURRENT_DOWNLOADS,
    FOLLOW_APP,
    MAX_CONCURRENT_DOWNLOADS,
    MAX_CONNECTIONS_PER_DOWNLOAD,
    MAX_QUEUE_DESCRIPTION_CHARS,
    ownSetting,
    ownValue,
    type QueueId,
    type QueueClaims,
    type QueueNotify,
    type QueueSettings,
    type Weekday,
  } from '@wirecatch/protocol';
  import { MB, SPEED_FIELD, splitKbOrMb } from '@wirecatch/units';
  import {
    Button,
    Callout,
    Checkbox,
    Dialog,
    Field,
    Input,
    NumberInput,
    SegmentedControl,
    ToggleChip,
    type FieldControl,
  } from '@wirecatch/ui';
  import { QUEUE_NOTIFY_OPTIONS, queueNotifyHelp } from '$lib/finish-notices';
  import { dayPattern, formatTime, WEEKDAY_SHORT } from '$lib/format';
  import { daysForPattern, pickedDaysHelp, toggledDay } from '$lib/schedule-days';
  import { FIRST_START, FIRST_STOP, readSchedule } from '$lib/queue-schedule';
  import { WEEKDAYS } from '$lib/time';
  import {
    concurrentLimitHelp,
    firstConcurrent,
    ownConnectionsHelp,
    queueConcurrentProblem,
    queueConnectionsProblem,
    queueDeletionNote,
    readQueueLimit,
    speedLimitHelp,
  } from '$lib/limits';
  import { claimsOf, takesOf } from '$lib/queue-claims';
  import { getApp } from '$lib/state/app.svelte';
  import ChoiceCard from './ChoiceCard.svelte';
  import QueueClaimsCard from './QueueClaimsCard.svelte';
  import QueueProxyCard from './QueueProxyCard.svelte';
  import SwitchCard from './SwitchCard.svelte';
  import TimeInput from './TimeInput.svelte';

  let { id, deleting = false }: { id: QueueId | null; deleting?: boolean } = $props();
  const app = getApp();

  // A snapshot of the queue when the dialog opened; the form edits its own copy.
  const original = (() => (id === null ? undefined : app.queues.byId(id)))();
  const opened = original?.settings;
  // What the form suggests when a limit or schedule is turned on for a queue that has none.
  // The core makes a new queue that follows the app; a limit switched on starts near the app's value.
  const FIRST_SPEED_LIMIT = 2 * MB;
  const formId = $props.id();

  const ownConcurrent = opened ? ownValue(opened.max_concurrent) : null;
  const ownLimit = opened ? ownValue(opened.speed_limit) : null;
  const ownConnections = opened ? ownValue(opened.connections) : null;

  let name = $state(opened?.name ?? '');
  let description = $state(opened?.description ?? '');
  let concurrentOn = $state(ownConcurrent !== null);
  let maxConcurrent = $state(
    ownConcurrent ??
      firstConcurrent(app.settings?.max_concurrent_downloads ?? DEFAULT_MAX_CONCURRENT_DOWNLOADS),
  );
  let limitOn = $state(ownLimit !== null);
  const initialLimit = splitKbOrMb(
    ownLimit ?? app.settings?.global_speed_limit ?? FIRST_SPEED_LIMIT,
  );
  let limitUnit = $state(initialLimit.unit);
  let limitValue = $state<number | null>(initialLimit.value);
  let scheduleOn = $state(opened !== undefined && opened.schedule !== null);
  let start = $state(formatTime(opened?.schedule?.start ?? FIRST_START));
  let untilEmpty = $state(opened?.schedule ? opened.schedule.stop === null : false);
  let stop = $state(formatTime(opened?.schedule?.stop ?? FIRST_STOP));
  let days = $state<Weekday[]>(opened?.schedule?.days ?? []);
  // Picking single days is its own choice, kept while the days picked happen to make a pattern.
  let pickingDays = $state(dayPattern(opened?.schedule?.days ?? []) === 'custom');
  let auto = $state(opened?.takes.kind === 'auto');
  // Kept while the form is switched to Manual, so switching back loses nothing.
  let claims = $state<QueueClaims>(claimsOf(opened?.takes));
  let notify = $state<QueueNotify>(opened?.notify ?? 'each_download');
  let proxy = $state<QueueSettings['proxy']>($state.snapshot(opened?.proxy) ?? FOLLOW_APP);
  let exceptions = $state<QueueSettings['exceptions']>(
    $state.snapshot(opened?.exceptions) ?? FOLLOW_APP,
  );
  const appConnections = app.settings?.connections_per_download ?? DEFAULT_CONNECTIONS_PER_DOWNLOAD;
  let connectionsOn = $state(ownConnections !== null);
  let connections = $state(ownConnections ?? appConnections);

  const concurrentHelp = $derived(
    concurrentLimitHelp(
      concurrentOn ? maxConcurrent : null,
      app.settings?.max_concurrent_downloads,
    ),
  );
  const limitHelp = $derived(speedLimitHelp(limitOn, app.settings?.global_speed_limit ?? null));
  const connectionsHelp = $derived(ownConnectionsHelp(connectionsOn, appConnections));

  const notifyHelp = $derived(queueNotifyHelp(notify, app.settings?.finish_notice));
  let error = $state('');
  // Opened to delete the queue (from the palette), it starts at that question.
  let confirmDelete = $state((() => deleting)());
  let busy = $state(false);

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

  const movingLine = $derived(
    original
      ? queueDeletionNote(
          app.downloads.idsInQueue(original.id).length,
          app.queues.defaultQueue?.settings.name ?? 'the default queue',
        )
      : '',
  );

  const pattern = $derived<DayPattern>(pickingDays ? 'custom' : dayPattern(days));
  function choosePattern(next: DayPattern) {
    pickingDays = next === 'custom';
    days = daysForPattern(next, days);
  }

  const daysHelp = $derived(pickedDaysHelp(days));

  /** What the form changes in a queue, or why it cannot. */
  function readForm(): QueueSettings | string {
    if (!name.trim()) return 'Give the queue a name.';
    const tooMany = concurrentOn ? queueConcurrentProblem(maxConcurrent) : null;
    if (tooMany !== null) return tooMany;
    const badConnections = connectionsOn ? queueConnectionsProblem(connections) : null;
    if (badConnections !== null) return badConnections;
    const limit = readQueueLimit(limitOn, limitValue, limitUnit);
    if (typeof limit === 'string') return limit;
    const schedule = readSchedule({
      on: scheduleOn,
      start,
      untilEmpty,
      stop,
      pickingDays,
      days,
    });
    if (typeof schedule === 'string') return schedule;
    return {
      name: name.trim(),
      description: description.trim(),
      max_concurrent: concurrentOn ? ownSetting(Math.round(maxConcurrent)) : FOLLOW_APP,
      speed_limit: limit.limit,
      connections: connectionsOn ? ownSetting(Math.round(connections)) : FOLLOW_APP,
      proxy: $state.snapshot(proxy),
      exceptions: $state.snapshot(exceptions),
      takes: takesOf(auto, $state.snapshot(claims)),
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
      const savedId = await app.queues.save(original?.id ?? null, changes);
      if (savedId === undefined) return;
      app.closeDialog();
      if (!original) app.show({ kind: 'queue', id: savedId });
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
  title={original ? `Edit ${original.settings.name}` : 'New queue'}
  description={original
    ? undefined
    : "A queue runs its downloads in order. It uses the app's limits unless you set its own."}
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
    <Field label="Name">
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
    <Field label="Description (optional)">
      {#snippet children(control: FieldControl)}
        <Input
          {...control}
          value={description}
          maxlength={MAX_QUEUE_DESCRIPTION_CHARS}
          onchange={(value: string) => (description = value)}
          placeholder="For example: Large files, overnight"
        />
      {/snippet}
    </Field>

    <h3 id="{formId}-limits" class="-mb-3 text-ui font-medium text-ink">Limits</h3>
    <section
      aria-labelledby="{formId}-limits"
      class="flex flex-col divide-y divide-line rounded-md border border-line bg-layer"
    >
      <SwitchCard
        flush
        label="Downloads at the same time"
        description={concurrentHelp}
        checked={concurrentOn}
        onchange={(on: boolean) => (concurrentOn = on)}
      >
        <NumberInput
          label="Downloads at the same time"
          class="w-28"
          value={maxConcurrent}
          min={1}
          max={MAX_CONCURRENT_DOWNLOADS}
          onchange={(value: number | null) => {
            if (value !== null) maxConcurrent = value;
          }}
        />
      </SwitchCard>

      <SwitchCard
        flush
        label="Speed limit"
        description={limitHelp}
        checked={limitOn}
        onchange={(on: boolean) => (limitOn = on)}
      >
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
      </SwitchCard>

      <SwitchCard
        flush
        label="Connections per download"
        description={connectionsHelp}
        checked={connectionsOn}
        onchange={(on: boolean) => (connectionsOn = on)}
      >
        <NumberInput
          label="Connections per download"
          class="w-28"
          value={connections}
          min={1}
          max={MAX_CONNECTIONS_PER_DOWNLOAD}
          onchange={(value: number | null) => {
            if (value !== null) connections = value;
          }}
        />
      </SwitchCard>
    </section>

    <QueueClaimsCard
      queue={original?.id ?? null}
      {name}
      {auto}
      {claims}
      onchange={(on: boolean, next: QueueClaims) => ([auto, claims] = [on, next])}
    />

    <QueueProxyCard
      {proxy}
      {exceptions}
      onchange={(p: QueueSettings['proxy'], e: QueueSettings['exceptions']) =>
        ([proxy, exceptions] = [p, e])}
    />

    <SwitchCard
      label="Only run at certain times"
      description="Outside these hours its downloads wait, unless you start the queue now."
      checked={scheduleOn}
      onchange={(on: boolean) => (scheduleOn = on)}
    >
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
                onchange={() => (days = toggledDay(days, d))}
              />
            {/each}
          </div>
          <p id="{formId}-days-help" class="text-small text-ink-muted">{daysHelp}</p>
        {/if}
      </div>
    </SwitchCard>

    <ChoiceCard
      label="Tell me when downloads finish"
      description={notifyHelp}
      value={notify}
      options={QUEUE_NOTIFY_OPTIONS}
      onchange={(next: QueueNotify) => (notify = next)}
    />

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
      <Button variant="danger" onclick={remove}>Delete {original.settings.name}</Button>
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
        {original ? 'Save' : 'Create queue'}
      </Button>
    {/if}
  {/snippet}
</Dialog>
