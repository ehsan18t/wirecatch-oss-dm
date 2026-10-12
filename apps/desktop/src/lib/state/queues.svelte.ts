// The queues: what each is doing right now, and creating, changing, pausing and deleting them.

import type { Queue, QueueId } from '@wirecatch/protocol';
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
    return (
      queue.schedule !== null &&
      !queue.running_outside_schedule &&
      !scheduleAllows(queue.schedule, this.#host.now())
    );
  }

  /** A short phrase for the sidebar: what this queue is doing right now. */
  status(queue: Queue): string {
    const c = this.#host.counts().queue.get(queue.id);
    if (queue.paused) return 'Paused';
    if (queue.schedule && this.isOutsideSchedule(queue)) {
      return `Starts at ${formatTime(queue.schedule.start)}`;
    }
    if (queue.running_outside_schedule) return 'Started by hand, outside its hours';
    if (c && c.running > 0) return `${c.running} of ${queue.max_concurrent} running`;
    if (c && c.waiting > 0) return `${c.waiting} waiting`;
    if (c && c.paused > 0) return `${c.paused} paused`;
    return 'Idle';
  }

  async create(name: string): Promise<Queue | undefined> {
    return this.#host.runner.run(() => this.#host.backend().createQueue(name));
  }

  update(queue: Queue): Promise<boolean> {
    return this.#host.runner.succeeds(() => this.#host.backend().updateQueue(queue));
  }

  /** Deletes a queue; its downloads move to the default queue. */
  async remove(id: QueueId): Promise<boolean> {
    const name = this.byId(id)?.name;
    const ok = await this.#host.runner.succeeds(() => this.#host.backend().deleteQueue(id));
    if (ok)
      this.#host.toasts.notify(
        `Deleted ${name ?? 'the queue'}. Its downloads moved to ${this.defaultQueue?.name ?? 'the default queue'}.`,
      );
    return ok;
  }

  /** Pauses or resumes a whole queue. Downloads paused one by one stay paused either way. */
  async setPaused(queue: QueueId, paused: boolean): Promise<void> {
    await this.#host.runner.run(() => this.#host.backend().setQueuePaused(queue, paused));
  }

  /** Runs a scheduled queue now, outside its hours, until it is empty or paused. */
  async startNow(queue: QueueId): Promise<void> {
    const ok = await this.#host.runner.succeeds(() => this.#host.backend().startQueueNow(queue));
    const name = this.byId(queue)?.name ?? 'The queue';
    if (ok)
      this.#host.toasts.notify(
        `${name} is running now. Its schedule applies again once it is empty.`,
      );
  }
}
