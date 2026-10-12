// Fills a new mock with sample downloads: the hand-written ones, then generated ones for load testing.

import { MB } from '@wirecatch/units';
import { MS_PER_HOUR, MS_PER_MINUTE } from '../../time';
import { MAIN_QUEUE_ID, OVERNIGHT_QUEUE_ID, WORK_QUEUE_ID } from './data';
import { between, type Spread } from './rng';
import { bulkItem, SEEDS } from './seeds';
import type { MockState } from './state';

/** A finished sample finished this many hours after it was added. */
const SEED_FINISHED_AFTER_HOURS = 0.2;

/** Generated downloads are this many megabytes, and go this fast in megabytes a second. */
const BULK_SIZE_MB: Spread = { min: 0.5, span: 800 };
const BULK_RATE_MB: Spread = { min: 0.3, span: 6 };
/** Every ninth generated download that is not running waits in its queue; the rest are finished. */
const BULK_QUEUED_EVERY = 9;
/** A waiting generated download is at most this far along. */
const BULK_MOST_DONE = 0.5;
/** Generated downloads take turns over the sample queues; the running ones are all in Main. */
const BULK_QUEUES = [MAIN_QUEUE_ID, OVERNIGHT_QUEUE_ID, WORK_QUEUE_ID];
/** Generated downloads were added a minute apart, and each finished half a minute after it was added. */
const BULK_ADDED_EVERY_MS = MS_PER_MINUTE;
const BULK_FINISHED_AFTER_MS = MS_PER_MINUTE / 2;

/** A load test runs every active download at once: the limits get this much room above them. */
const LOAD_TEST_MIN_GLOBAL_LIMIT = 4;
const LOAD_TEST_GLOBAL_ROOM = 8;
const LOAD_TEST_QUEUE_ROOM = 4;

/** Adds `count` downloads, of which the generated ones beyond the samples have `active` running. */
export function populate(state: MockState, count: number, active: number): void {
  addSamples(state, count);
  addGenerated(state, count, active);
  if (active > 0) makeRoomFor(state, active);
}

function addSamples(state: MockState, count: number) {
  const now = state.now();
  for (const s of SEEDS.slice(0, count)) {
    state.insert(
      {
        url: s.url,
        file_name: s.name,
        queue: s.queue,
        status: s.status,
        total: s.total === null ? null : Math.round(s.total),
        done: s.total === null ? 0 : Math.round(s.total * s.fraction),
        error: s.error ?? null,
        added_at: Math.round(now - s.age * MS_PER_HOUR),
        completed_at:
          s.status === 'completed'
            ? Math.round(now - (s.age - SEED_FINISHED_AFTER_HOURS) * MS_PER_HOUR)
            : null,
      },
      s.rate,
    );
  }
}

/** Generated rows for load testing: mostly finished, some waiting, `active` running. */
function addGenerated(state: MockState, count: number, active: number) {
  const now = state.now();
  for (let i = SEEDS.length; i < count; i++) {
    const { name, url } = bulkItem(i);
    const total = Math.round(between(state.random, BULK_SIZE_MB) * MB);
    const isActive = i - SEEDS.length < active;
    const status = isActive ? 'downloading' : i % BULK_QUEUED_EVERY === 0 ? 'queued' : 'completed';
    const addedAt = now - i * BULK_ADDED_EVERY_MS;
    const done =
      status === 'completed' ? total : Math.round(total * state.random() * BULK_MOST_DONE);
    state.insert(
      {
        url,
        file_name: name,
        queue: isActive ? MAIN_QUEUE_ID : (BULK_QUEUES[i % BULK_QUEUES.length] ?? MAIN_QUEUE_ID),
        status,
        total,
        done,
        added_at: addedAt,
        completed_at: status === 'completed' ? addedAt + BULK_FINISHED_AFTER_MS : null,
      },
      between(state.random, BULK_RATE_MB) * MB,
    );
  }
}

function makeRoomFor(state: MockState, active: number) {
  state.settings.max_concurrent_downloads = Math.max(
    LOAD_TEST_MIN_GLOBAL_LIMIT,
    active + LOAD_TEST_GLOBAL_ROOM,
  );
  const main = state.queues.find((q) => q.is_default);
  if (main) main.max_concurrent = Math.max(main.max_concurrent, active + LOAD_TEST_QUEUE_ROOM);
}
