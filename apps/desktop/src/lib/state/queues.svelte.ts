// The queues: what each is doing right now, and creating, changing, pausing and deleting them.

import { ownValue, type Queue, type QueueId, type QueueSettings } from '@wirecatch/protocol';
import type { Backend } from '../backend';
import { formatTime, scheduleAllows } from '../format';
import type { Runner } from './runner';
import type { Toasts } from './toasts.svelte';
import type { Counts } from './view';

/** What the queues need from the window they are shown in. */
export interface QueuesHost {
  backend: () => Backend;
  runner: Runner;
  toasts: Toasts;
  /** The clock schedule labels follow. */
  now: () => Date;
  /** How many downloads of each queue are in each state. */
  counts: () => Counts;
}

/** How many of a queue's downloads run, out of its own limit when it has one. */
function runningPhrase(running: number, atOnce: number | null): string {
  return atOnce === null
    ? `${String(running)} running`
    : `${String(running)} of ${String(atOnce)} running`;
}

export class Queues {
  list = $state<Queue[]>([]);
  readonly #host: QueuesHost;

  constructor(host: QueuesHost) {
    this.#host = host;
  }

  /** The queue new downloads go to. */
  defaultQueue = $derived(this.list.find((q) => q.is_default) ?? this.list[0]);

  byId(id: QueueId): Queue | undefined {
    return this.list.find((q) => q.id === id);
  }

  /** True when a scheduled queue is outside its hours and not started by hand. */
  isOutsideSchedule(queue: Queue): boolean {
    const { schedule } = queue.settings;
    return (
      schedule !== null &&
      !queue.status.running_outside_schedule &&
      !scheduleAllows(schedule, this.#host.now())
    );
  }

  /** A short phrase for the sidebar: what this queue is doing right now, and when idle how many it may run at once when it has its own limit. */
  describe(queue: Queue): string {
    const c = this.#host.counts().queue.get(queue.id);
    const { schedule } = queue.settings;
    const atOnce = ownValue(queue.settings.max_concurrent);
    if (queue.status.paused) return 'Paused';
    if (schedule && this.isOutsideSchedule(queue)) {
      return `Starts at ${formatTime(schedule.start)}`;
    }
    if (queue.status.running_outside_schedule) return 'Started by hand, outside its hours';
    if (c && c.running > 0) return runningPhrase(c.running, atOnce);
    if (c && c.waiting > 0) return `${c.waiting} waiting`;
    if (c && c.paused > 0) return `${c.paused} paused`;
    return atOnce === null ? 'Idle' : `Idle, up to ${String(atOnce)} at once`;
  }

  /** `id` `null` makes a new queue; `undefined` when the runner showed the failure. */
  async save(id: QueueId | null, settings: QueueSettings): Promise<QueueId | undefined> {
    const backend = this.#host.backend();
    if (id === null) return (await this.#host.runner.run(() => backend.createQueue(settings)))?.id;
    const saved = await this.#host.runner.succeeds(() => backend.updateQueue(id, settings));
    return saved ? id : undefined;
  }

  /** Deletes a queue; its downloads move to the default queue. */
  async remove(id: QueueId): Promise<boolean> {
    const name = this.byId(id)?.settings.name;
    const ok = await this.#host.runner.succeeds(() => this.#host.backend().deleteQueue(id));
    if (ok)
      this.#host.toasts.notify(
        `Deleted ${name ?? 'the queue'}. Its downloads moved to ${this.defaultQueue?.settings.name ?? 'the default queue'}.`,
      );
    return ok;
  }

  /** Moves a queue to `index` in the sidebar, whose order also settles ties between claims; the default queue stays first (D72.9). */
  async moveTo(queue: QueueId, index: number): Promise<void> {
    const order = this.list.map((q) => q.id);
    const from = order.indexOf(queue);
    const to = Math.min(Math.max(index, 1), order.length - 1);
    if (from < 1 || from === to) return;
    order.splice(from, 1);
    order.splice(to, 0, queue);
    await this.#host.runner.run(() => this.#host.backend().setQueueOrder(order));
  }

  /** Pauses or resumes a whole queue. Downloads paused one by one stay paused either way. */
  async setPaused(queue: QueueId, paused: boolean): Promise<void> {
    await this.#host.runner.run(() => this.#host.backend().setQueuePaused(queue, paused));
  }

  /** Runs a scheduled queue now, outside its hours, until it is empty or paused. */
  async startNow(queue: QueueId): Promise<void> {
    const ok = await this.#host.runner.succeeds(() => this.#host.backend().startQueueNow(queue));
    const name = this.byId(queue)?.settings.name ?? 'The queue';
    if (ok)
      this.#host.toasts.notify(
        `${name} is running now. Its schedule applies again once it is empty.`,
      );
  }
}
