<script lang="ts" module>
  import type { SelectOption } from '@wirecatch/ui';

  const UNITS: readonly SelectOption<'KB' | 'MB'>[] = [
    { value: 'KB', label: 'KB/s' },
    { value: 'MB', label: 'MB/s' },
  ];

  // "At" comes last, next to the time it uses.
  const STOPS: readonly SelectOption<'time' | 'empty'>[] = [
    { value: 'empty', label: 'When the queue is empty' },
    { value: 'time', label: 'At' },
  ];
</script>

<script lang="ts">
  // A queue's name and description, its own limits, its hours and when it notifies you: each limit is a switch that opens its own fields, and off follows the global value (D72.1); labels and help never change with what is chosen (D84).
  import {
    DEFAULT_CONNECTIONS_PER_DOWNLOAD,
    DEFAULT_MAX_CONCURRENT_DOWNLOADS,
    GLOBAL,
    MAX_CONCURRENT_DOWNLOADS,
    MAX_CONNECTIONS_PER_DOWNLOAD,
    MAX_QUEUE_DESCRIPTION_CHARS,
    ownSetting,
    ownValue,
    type QueueId,
    type QueueRules,
    type QueueNotify,
    type QueueSettings,
    type Weekday,
  } from '@wirecatch/protocol';
  import { MB, SPEED_FIELD, splitKbOrMb } from '@wirecatch/units';
  import {
    Button,
    Callout,
    Dialog,
    Field,
    Input,
    NumberInput,
    SegmentedControl,
    type FieldControl,
  } from '@wirecatch/ui';
  import { QUEUE_NOTIFY_HELP, QUEUE_NOTIFY_OPTIONS, queueNotifyWarning } from '$lib/notify-options';
  import { formatTime } from '$lib/format';
  import {
    FIRST_START,
    FIRST_STOP,
    NO_DAYS,
    pressedDays,
    readSchedule,
    toggledDay,
  } from '$lib/queue-schedule';
  import {
    concurrentCapWarning,
    firstConcurrent,
    globalLimitText,
    queueConcurrentProblem,
    queueConnectionsProblem,
    queueDeletionNote,
    readQueueLimit,
  } from '$lib/limits';
  import { rulesOf, addDownloadsOf } from '$lib/queue-rules';
  import { getApp } from '$lib/state/app.svelte';
  import ChoiceCard from './ChoiceCard.svelte';
  import DayPicker from './DayPicker.svelte';
  import QueueRulesCard from './QueueRulesCard.svelte';
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
  let days = $state<Weekday[]>(pressedDays(opened?.schedule?.days));
  let auto = $state(opened?.add_downloads.kind === 'auto');
  // Kept while adding automatically is off, so turning it back on loses nothing.
  let rules = $state<QueueRules>(rulesOf(opened?.add_downloads));
  let notify = $state<QueueNotify>(opened?.notify ?? 'each_download');
  let proxy = $state<QueueSettings['proxy']>($state.snapshot(opened?.proxy) ?? GLOBAL);
  let exceptions = $state<QueueSettings['exceptions']>(
    $state.snapshot(opened?.exceptions) ?? GLOBAL,
  );
  const appConnections = app.settings?.connections_per_download ?? DEFAULT_CONNECTIONS_PER_DOWNLOAD;
  let connectionsOn = $state(ownConnections !== null);
  let connections = $state(ownConnections ?? appConnections);

  const concurrentCap = $derived(
    concurrentCapWarning(maxConcurrent, app.settings?.max_concurrent_downloads),
  );
  let error = $state('');
  // Opened to delete the queue (from the palette), it starts at that question.
  let confirmDelete = $state((() => deleting)());
  let busy = $state(false);

  // The button pressed goes as the footer changes: the focus moves to the safe answer, and back to Delete queue on Keep queue.
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
      days,
    });
    if (typeof schedule === 'string') return schedule;
    return {
      name: name.trim(),
      description: description.trim(),
      max_concurrent: concurrentOn ? ownSetting(Math.round(maxConcurrent)) : GLOBAL,
      speed_limit: limit.limit,
      connections: connectionsOn ? ownSetting(Math.round(connections)) : GLOBAL,
      proxy: $state.snapshot(proxy),
      exceptions: $state.snapshot(exceptions),
      add_downloads: addDownloadsOf(auto, $state.snapshot(rules)),
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
        help="Up to this many of its downloads run at once, and the rest wait. The global limit still applies across all queues."
        inherited={globalLimitText('max_concurrent', app.settings?.max_concurrent_downloads)}
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
        {#if concurrentCap}<p class="text-small text-warning">{concurrentCap}</p>{/if}
      </SwitchCard>

      <SwitchCard
        flush
        label="Speed limit"
        help="Shared by all downloads in this queue. The global limit still applies."
        inherited={globalLimitText('speed_limit', app.settings?.global_speed_limit)}
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
        help="Each download in this queue uses up to this many connections, in place of the global setting."
        inherited={globalLimitText('connections', app.settings?.connections_per_download)}
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

    <QueueRulesCard
      queue={original?.id ?? null}
      {name}
      {auto}
      {rules}
      onchange={(on: boolean, next: QueueRules) => ([auto, rules] = [on, next])}
    />

    <QueueProxyCard
      queueName={name}
      {proxy}
      {exceptions}
      onchange={(p: QueueSettings['proxy'], e: QueueSettings['exceptions']) =>
        ([proxy, exceptions] = [p, e])}
    />

    <SwitchCard
      label="Run on a schedule"
      help="Its downloads start only between these times, on the days picked. A queue you start manually ignores its schedule."
      checked={scheduleOn}
      onchange={(on: boolean) => (scheduleOn = on)}
    >
      <div class="flex flex-col gap-2.5">
        <div class="flex items-center gap-3">
          <label for="{formId}-start" class="w-20 shrink-0 text-ui text-ink">Starts at</label>
          <TimeInput
            id="{formId}-start"
            value={start}
            onchange={(value: string) => (start = value)}
          />
        </div>
        <div class="flex items-center gap-3">
          <span class="w-20 shrink-0 text-ui text-ink" aria-hidden="true">Stops</span>
          <div class="flex flex-wrap items-center gap-2">
            <SegmentedControl
              label="Stops"
              size="sm"
              value={untilEmpty ? 'empty' : 'time'}
              options={STOPS}
              onchange={(next: 'time' | 'empty') => (untilEmpty = next === 'empty')}
            />
            {#if !untilEmpty}
              <TimeInput
                label="Stop time"
                value={stop}
                onchange={(value: string) => (stop = value)}
              />
            {/if}
          </div>
        </div>
        <div class="flex items-start gap-3">
          <span class="w-20 shrink-0 pt-1 text-ui text-ink" aria-hidden="true">Days</span>
          <div class="flex min-w-0 flex-1 flex-col gap-1">
            <DayPicker
              {days}
              invalid={days.length === 0}
              describedby={days.length === 0 ? `${formId}-days-error` : undefined}
              onchange={(day: Weekday) => (days = toggledDay(days, day))}
            />
            {#if days.length === 0}
              <p id="{formId}-days-error" class="text-small text-danger">{NO_DAYS}</p>
            {/if}
          </div>
        </div>
      </div>
    </SwitchCard>

    <ChoiceCard
      label="Notify me when downloads finish"
      help={QUEUE_NOTIFY_HELP}
      warning={queueNotifyWarning(app.settings?.notify_on_finish)}
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
        >Keep queue</Button
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
