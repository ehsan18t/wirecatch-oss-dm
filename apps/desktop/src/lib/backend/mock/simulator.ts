// The simulation step: decides what runs and why the rest waits, moves bytes within the speed
// limits, finishes joining and converting, and moves add-on installs along.

import {
  PER_MILLE_DONE,
  type DownloadId,
  type DownloadSummary,
  type Queue,
  type QueueId,
  type WaitReason,
} from '@wirecatch/protocol';
import { MB } from '@wirecatch/units';
import { scheduleAllows } from '../../format';
import { between, type Spread } from './rng';
import { RUNNING, stop, type AddonPage, type MockState, type Sim } from './state';

/** How often the simulation steps on its own, in milliseconds. */
export const DEFAULT_TICK_MS = 250;
/** How long a download spends connecting before bytes arrive. */
const CONNECTING_MS = 700;
/** Each step a download goes this fraction of its rate: a little slower or faster, as real ones do. */
const WOBBLE: Spread = { min: 0.75, span: 0.5 };
/** How much a new speed counts against the last one, so numbers do not jump around (as the core smooths). */
const SPEED_SMOOTHING = 0.4;
/** Connections a download with a known size opens, within the setting; one with no size opens one. */
const SIMULATED_CONNECTIONS = 8;
const MS_PER_SECOND = 1000;
/** How long joining video and audio takes after a media download; converting takes three times as long. */
const PROCESSING_MS = 2500;
const CONVERTING_TAKES = 3;
/** How long an add-on install takes from start to finish. */
const INSTALL_MS = 4000;
/** The download size of an add-on that does not say. */
const ADDON_FALLBACK_SIZE = 96 * MB;

/** One step of the simulation: what it changes, and how long the step is. */
interface Step {
  state: MockState;
  /** Milliseconds since the last step. */
  dt: number;
  /** The time of this step. */
  now: number;
  /** Rows this step changed, in the order they first changed. */
  changed: Map<DownloadId, DownloadSummary>;
}

/** A queue's line during scheduling. */
interface Line {
  queue: Queue;
  /** Whether its schedule (or "Start now") lets it run. */
  inWindow: boolean;
  /** How many more of its downloads may run. */
  room: number;
  /** How many of its downloads wait so far. */
  waiting: number;
}

/** Room left under the global limit of concurrent downloads. */
interface GlobalRoom {
  max: number;
  room: number;
}

/**
 * Advances the simulation by `dt` milliseconds. Returns the pages that waited for an add-on whose
 * install just finished: they continue on their own, under the same id.
 */
export function step(state: MockState, dt: number): AddonPage[] {
  const s: Step = { state, dt, now: state.now(), changed: new Map() };
  const running = schedule(s);
  const wanted = wantedSpeeds(s, running);
  moveBytes(s, wanted);
  advanceProcessing(s, wanted);
  state.changed([...s.changed.values()]);
  return advanceInstalls(state, dt);
}

function sameWait(a: WaitReason | null, b: WaitReason | null): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

function setWait(s: Step, row: DownloadSummary, waiting: WaitReason | null) {
  if (!sameWait(row.waiting, waiting)) {
    row.waiting = waiting;
    s.changed.set(row.id, row);
  }
}

/** Decides what runs: each queue in order, up to its own limit and the global limit. */
function schedule(s: Step): Map<QueueId, DownloadSummary[]> {
  const { state } = s;
  const date = new Date(s.now);
  const max = state.settings.max_concurrent_downloads;
  const global: GlobalRoom = { max, room: max };
  const running = new Map<QueueId, DownloadSummary[]>();
  let queuesChanged = false;
  for (const queue of state.queues) {
    const inWindow = queue.running_outside_schedule || scheduleAllows(queue.schedule, date);
    const line: Line = {
      queue,
      inWindow,
      room: !queue.paused && inWindow ? queue.max_concurrent : 0,
      waiting: 0,
    };
    const { list, active } = scheduleLine(s, line, global);
    // "Start now" lasts until the queue runs out of work.
    if (queue.running_outside_schedule && active === 0) {
      queue.running_outside_schedule = false;
      queuesChanged = true;
    }
    running.set(queue.id, list);
  }
  if (queuesChanged) state.queuesChanged();
  return running;
}

/** Runs what fits of one queue's downloads, and says why everything else waits. */
function scheduleLine(s: Step, line: Line, global: GlobalRoom) {
  const list: DownloadSummary[] = [];
  let active = 0;
  for (const id of s.state.order) {
    const row = s.state.rows.get(id);
    if (row?.queue !== line.queue.id) continue;
    if (row.status !== 'queued' && !RUNNING.has(row.status)) {
      if (row.waiting !== null) setWait(s, row, null);
      continue;
    }
    active += 1;
    if (line.room > 0 && global.room > 0) {
      line.room -= 1;
      global.room -= 1;
      list.push(row);
      run(s, row);
    } else {
      hold(s, row, line, global.max);
    }
  }
  return { list, active };
}

function run(s: Step, row: DownloadSummary) {
  setWait(s, row, null);
  if (row.status !== 'queued') return;
  row.status = 'connecting';
  const sim = s.state.sim.get(row.id);
  if (sim) sim.connectingMs = 0;
  s.changed.set(row.id, row);
}

function hold(s: Step, row: DownloadSummary, line: Line, globalMax: number) {
  if (row.status !== 'queued') {
    row.status = 'queued';
    stop(row);
    s.changed.set(row.id, row);
  }
  line.waiting += 1;
  setWait(s, row, waitReason(line, globalMax));
}

function waitReason({ queue, inWindow, room, waiting }: Line, globalMax: number): WaitReason {
  if (queue.paused) return { type: 'queue_paused' };
  if (!inWindow)
    return { type: 'queue_schedule', data: { starts_at: queue.schedule?.start ?? null } };
  if (waiting > 1) return { type: 'in_line', data: { position: waiting } };
  if (room <= 0) {
    return {
      type: 'queue_full',
      data: { running: queue.max_concurrent, limit: queue.max_concurrent },
    };
  }
  return { type: 'global_limit', data: { running: globalMax, limit: globalMax } };
}

/** How fast each running download would go, within its queue's speed limit. */
function wantedSpeeds(s: Step, running: Map<QueueId, DownloadSummary[]>): Map<DownloadId, number> {
  const wanted = new Map<DownloadId, number>();
  for (const q of s.state.queues) {
    const list = running.get(q.id) ?? [];
    const downloading = list.filter((r) => r.status === 'downloading');
    const share =
      q.speed_limit === null ? Infinity : q.speed_limit / Math.max(1, downloading.length);
    for (const row of list) {
      const sim = s.state.sim.get(row.id);
      if (!sim) continue;
      if (row.status === 'connecting') connect(s, row, sim);
      else wanted.set(row.id, Math.min(share, sim.rate * between(s.state.random, WOBBLE)));
    }
  }
  return wanted;
}

function connect(s: Step, row: DownloadSummary, sim: Sim) {
  sim.connectingMs += s.dt;
  if (sim.connectingMs >= CONNECTING_MS) {
    row.status = 'downloading';
    s.changed.set(row.id, row);
  }
}

/** Moves bytes at the wanted speeds, scaled down to the global speed limit. */
function moveBytes(s: Step, wanted: Map<DownloadId, number>) {
  let total = 0;
  for (const v of wanted.values()) total += v;
  const limit = s.state.settings.global_speed_limit;
  const scale = limit !== null && total > limit ? limit / total : 1;
  for (const [id, want] of wanted) {
    const row = s.state.rows.get(id);
    if (!row) continue;
    download(s, row, Math.round(want * scale));
    s.changed.set(id, row);
  }
}

function download(s: Step, row: DownloadSummary, speed: number) {
  row.speed =
    row.speed === 0
      ? speed
      : Math.round(row.speed * (1 - SPEED_SMOOTHING) + speed * SPEED_SMOOTHING);
  row.connections = Math.min(
    s.state.settings.connections_per_download,
    row.total === null ? 1 : SIMULATED_CONNECTIONS,
  );
  row.done += Math.round((speed * s.dt) / MS_PER_SECOND);
  if (row.total !== null && row.done >= row.total) {
    row.done = row.total;
    finishDownload(s, row);
    stop(row);
  } else {
    row.eta_secs =
      row.total === null || row.speed === 0 ? null : Math.round((row.total - row.done) / row.speed);
  }
}

/** Media with separate video and audio is joined (or converted) before it counts as finished. */
function finishDownload(s: Step, row: DownloadSummary) {
  const sim = s.state.sim.get(row.id);
  if (sim && (sim.needsJoin || sim.step !== null)) {
    row.status = 'processing';
    sim.processingMs = 0;
    row.processing = { step: sim.step ?? 'joining', per_mille: sim.step ? 0 : null };
  } else {
    row.status = 'completed';
    row.completed_at = s.now;
  }
}

/** Joining video and audio takes a moment, then the file is finished. */
function advanceProcessing(s: Step, wanted: Map<DownloadId, number>) {
  for (const [id, sim] of s.state.sim) {
    const row = s.state.rows.get(id);
    if (row?.status !== 'processing' || wanted.has(id)) continue;
    processRow(s, row, sim);
  }
}

function processRow(s: Step, row: DownloadSummary, sim: Sim) {
  sim.processingMs += s.dt;
  // Converting takes longer than joining, and reports how far it is.
  const takes = sim.step === 'converting' ? PROCESSING_MS * CONVERTING_TAKES : PROCESSING_MS;
  if (sim.processingMs >= takes) {
    row.status = 'completed';
    row.completed_at = s.now;
    row.processing = null;
    s.changed.set(row.id, row);
  } else if (row.processing && row.processing.per_mille !== null) {
    // Short of done until the step ends, as the core reports it.
    row.processing = {
      step: row.processing.step,
      per_mille: Math.min(
        PER_MILLE_DONE - 1,
        Math.floor((sim.processingMs / takes) * PER_MILLE_DONE),
      ),
    };
    s.changed.set(row.id, row);
  }
}

/** Moves simulated add-on installs along; returns the pages waiting for one that finished. */
function advanceInstalls(state: MockState, dt: number): AddonPage[] {
  let changed = false;
  let resumed: AddonPage[] = [];
  for (const addon of state.addons) {
    if (addon.state.type !== 'installing') continue;
    const total = addon.state.data.total ?? ADDON_FALLBACK_SIZE;
    const done = Math.min(total, addon.state.data.done + Math.round((total * dt) / INSTALL_MS));
    changed = true;
    if (done < total) {
      addon.state = { type: 'installing', data: { done, total } };
      continue;
    }
    addon.state = { type: 'installed' };
    addon.installed_version = addon.latest_version;
    resumed = [...resumed, ...state.waitingForAddon];
    state.waitingForAddon = [];
  }
  if (changed) state.addonsChanged();
  return resumed;
}
