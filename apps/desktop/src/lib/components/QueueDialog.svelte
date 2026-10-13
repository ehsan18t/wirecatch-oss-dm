<script lang="ts">
  import {
    MAX_CONCURRENT_DOWNLOADS,
    NEW_QUEUE_MAX_CONCURRENT,
    type Queue,
    type QueueId,
    type TimeOfDay,
    type Weekday,
  } from '@wirecatch/protocol';
  import { MB, splitKbOrMb, toBytes } from '@wirecatch/units';
  import { formatDays, formatTime, parseTime, WEEKDAY_SHORT } from '$lib/format';
  import { DAYS_PER_WEEK, WEEKDAYS } from '$lib/time';
  import { queueConcurrentProblem } from '$lib/limits';
  import { getApp } from '$lib/state/app.svelte';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';

  let { id }: { id: QueueId | null } = $props();
  const app = getApp();

  // A snapshot of the queue when the dialog opened; the form edits its own copy.
  const original = (() => (id === null ? undefined : app.queues.byId(id)))();
  // What the form suggests when a limit or schedule is turned on for a queue that has none.
  // The core makes a new queue without either; these are only where the fields start.
  const FIRST_SPEED_LIMIT = 2 * MB;
  const FIRST_START: TimeOfDay = { hour: 1, minute: 0 };
  const FIRST_STOP: TimeOfDay = { hour: 7, minute: 0 };

  let name = $state(original?.name ?? '');
  let maxConcurrent = $state(original?.max_concurrent ?? NEW_QUEUE_MAX_CONCURRENT);
  let limitOn = $state(original !== undefined && original.speed_limit !== null);
  const initialLimit = splitKbOrMb(original?.speed_limit ?? FIRST_SPEED_LIMIT);
  let limitUnit = $state(initialLimit.unit);
  let limitValue = $state(initialLimit.value);
  let scheduleOn = $state(original !== undefined && original.schedule !== null);
  let start = $state(formatTime(original?.schedule?.start ?? FIRST_START));
  let stopOn = $state(original?.schedule ? original.schedule.stop !== null : true);
  let stop = $state(formatTime(original?.schedule?.stop ?? FIRST_STOP));
  let days = $state<Weekday[]>(original?.schedule?.days ?? []);
  let error = $state('');
  let confirmDelete = $state(false);
  let busy = $state(false);
  /** A queue this dialog already created, so a retried save updates it instead of making another. */
  let created = $state<Queue>();

  // Picking days limits the queue to those days; picking none means every day.
  function toggleDay(d: Weekday) {
    days = days.includes(d) ? days.filter((x) => x !== d) : [...days, d];
  }

  const daysHelp = $derived.by(() => {
    if (days.length === 0 || days.length === DAYS_PER_WEEK)
      return 'No days picked, so it runs every day.';
    const words = formatDays(days);
    return `Runs only on ${words === 'Weekdays' || words === 'Weekends' ? words.toLowerCase() : words}.`;
  });

  /** The speed limit the form describes, or why it is not one. */
  function readLimit(): { limit: Queue['speed_limit'] } | string {
    if (!limitOn) return { limit: null };
    const limit = toBytes(limitValue, limitUnit);
    return limit === null || limit <= 0
      ? 'Enter a speed limit above zero, or turn the limit off.'
      : { limit };
  }

  /** The schedule the form describes, or why it is not one. */
  function readSchedule(): { schedule: Queue['schedule'] } | string {
    if (!scheduleOn) return { schedule: null };
    const s = parseTime(start);
    const e = stopOn ? parseTime(stop) : null;
    if (!s) return `Enter a start time like ${formatTime(FIRST_START)}.`;
    if (stopOn && !e) return `Enter a stop time like ${formatTime(FIRST_STOP)}.`;
    if (e?.hour === s.hour && e.minute === s.minute)
      return 'The start and stop times are the same. Pick a different stop time.';
    // Seven ticked days is the same as every day.
    const every = days.length === DAYS_PER_WEEK;
    return {
      schedule: { start: s, stop: e, days: every ? [] : WEEKDAYS.filter((d) => days.includes(d)) },
    };
  }

  /** What the form changes in a queue, or why it cannot. */
  function readForm():
    Pick<Queue, 'name' | 'max_concurrent' | 'speed_limit' | 'schedule'> | string {
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
      let base = original ?? created;
      if (!base) {
        base = await app.queues.create(changes.name);
        if (!base) return;
        created = base;
      }
      if (await app.queues.update({ ...base, ...changes })) {
        app.closeDialog();
        if (!original) app.show({ kind: 'queue', id: base.id });
      } else if (created) {
        error = 'The queue was made, but its settings were not saved. Save again to retry.';
      }
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!original) return;
    if (!confirmDelete) {
      confirmDelete = true;
      return;
    }
    if (await app.queues.remove(original.id)) app.closeDialog();
  }
</script>

<Dialog
  title={original ? `Edit ${original.name}` : 'New queue'}
  description="A queue runs its downloads in order, with its own limits and hours."
  onclose={() => app.closeDialog()}
  size="medium"
>
  <form
    id="queue-form"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    <label class="form-stack">
      <span class="form-label">Name</span>
      <input class="field" bind:value={name} placeholder="For example: Overnight" required />
    </label>

    <div class="form-stack">
      <label class="form-label" for="queue-max">Downloads at the same time</label>
      <input
        id="queue-max"
        class="field narrow"
        type="number"
        min="1"
        max={MAX_CONCURRENT_DOWNLOADS}
        aria-describedby="queue-max-help"
        bind:value={maxConcurrent}
      />
      <span id="queue-max-help" class="form-help">The rest wait their turn in order.</span>
    </div>

    <fieldset>
      <legend class="visually-hidden">Speed limit</legend>
      <label class="form-check">
        <input type="checkbox" bind:checked={limitOn} />
        <span class="form-label">Limit this queue's speed</span>
      </label>
      {#if limitOn}
        <div class="inline">
          <input
            class="field narrow"
            type="number"
            min="0.1"
            step="0.1"
            aria-label="Speed limit"
            bind:value={limitValue}
          />
          <select class="field" aria-label="Unit" bind:value={limitUnit}>
            <option value="KB">KB/s</option>
            <option value="MB">MB/s</option>
          </select>
          <span class="form-help">Shared by everything running in this queue.</span>
        </div>
      {/if}
    </fieldset>

    <fieldset>
      <legend class="visually-hidden">Schedule</legend>
      <label class="form-check">
        <input type="checkbox" bind:checked={scheduleOn} />
        <span class="form-label">Only run at certain times</span>
      </label>
      {#if scheduleOn}
        <div class="inline">
          <label class="inline">From <input class="field" type="time" bind:value={start} /></label>
          <label class="form-check">
            <input type="checkbox" bind:checked={stopOn} />
            <span>until</span>
          </label>
          {#if stopOn}
            <input class="field" type="time" aria-label="Stop time" bind:value={stop} />
          {:else}
            <span class="form-help">the queue is empty</span>
          {/if}
        </div>
        <div class="days-row">
          <span class="form-label" id="queue-days-label">Only on these days</span>
          <div
            class="days"
            role="group"
            aria-labelledby="queue-days-label"
            aria-describedby="queue-days-help"
          >
            {#each WEEKDAYS as d (d)}
              <button
                type="button"
                class="day"
                aria-pressed={days.includes(d)}
                onclick={() => toggleDay(d)}
              >
                {WEEKDAY_SHORT[d]}
              </button>
            {/each}
          </div>
          <span id="queue-days-help" class="form-help">{daysHelp}</span>
        </div>
      {/if}
    </fieldset>

    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </form>

  {#snippet footer()}
    {#if original && !original.is_default}
      <Button variant="danger" class="queue-delete" onclick={remove}>
        {confirmDelete
          ? `Delete it; downloads move to ${app.queues.defaultQueue?.name ?? 'the default queue'}`
          : 'Delete queue'}
      </Button>
    {/if}
    <Button onclick={() => app.closeDialog()}>Cancel</Button>
    <Button type="submit" form="queue-form" variant="primary" disabled={busy}>
      {original || created ? 'Save' : 'Create queue'}
    </Button>
  {/snippet}
</Dialog>

<style>
  form {
    display: grid;
    gap: 16px;
  }

  fieldset {
    display: grid;
    gap: 8px;
    margin: 0;
    padding: 0;
    border: 0;
  }

  .narrow {
    width: 90px;
  }

  .inline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    font-size: var(--text-sm);
  }

  .days-row {
    display: grid;
    gap: 4px;
  }

  .days {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
  }

  .day {
    width: 42px;
    height: 28px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-s);
    background: var(--surface);
    color: var(--ink-2);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .day[aria-pressed='true'] {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--ink);
    font-weight: 600;
  }

  .error {
    margin: 0;
    color: var(--danger);
    font-size: var(--text-sm);
  }

  /* The delete button stands apart from Cancel and Save, at the start of the bar. */
  :global(.queue-delete) {
    margin-right: auto;
  }
</style>
