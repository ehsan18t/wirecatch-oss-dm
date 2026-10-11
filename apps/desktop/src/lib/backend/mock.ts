// An in-memory stand-in for the core, so the interface can be built, reviewed and tested in a
// normal browser. It simulates queues, limits, schedules, varied speeds, failures with fixes,
// and browser captures, and pushes the same `UiEvent`s the real core will.

import type {
  AddonInfo,
  AddRequest,
  AppSettings,
  CaptureConfirmation,
  DownloadId,
  DownloadSummary,
  MediaChoice,
  MediaSelection,
  PendingCapture,
  Queue,
  QueueId,
  SettingsProfile,
  UiEvent,
  WaitReason,
} from '@wirecatch/protocol';
import { isProgramName } from '../filetypes';
import { fileNameFromUrl, folderOf, hostOf, scheduleAllows } from '../format';
import {
  bulkItem,
  categoryFor,
  defaultQueues,
  defaultSettings,
  DOWNLOADS,
  folderFor,
  HOME,
  SEEDS,
} from './mock-data';
import {
  BackendError,
  type AddonRequestItem,
  type Backend,
  type SampleData,
  type ShellSignal,
  type Unsubscribe,
} from './types';

/** The one optional add-on (D20): yt-dlp, ffmpeg and Deno, installed together. */
export function mediaAddon(): AddonInfo {
  return {
    id: 'media',
    // As the core describes it.
    name: 'Media add-on',
    description:
      'Downloads video and audio from sites such as YouTube (yt-dlp), and joins separate video and audio into one file (ffmpeg).',
    state: { type: 'not_installed' },
    installed_version: null,
    latest_version: 'yt-dlp 2026.08.19, ffmpeg 9.0.2, Deno 2.9.7',
    size: 131 * 1024 * 1024,
  };
}

/** Sample media pages: a title, its page, and the qualities it offers, best first. */
const MEDIA_SAMPLES: ReadonlyArray<{ title: string; page: string; source: 'hls' | 'page' }> = [
  {
    title: 'Northern lights over Tromso, 4K time-lapse',
    page: 'https://video.example.com/watch/aurora-tromso',
    source: 'page',
  },
  {
    title: 'Conference talk: fast downloads explained',
    page: 'https://talks.example.org/2026/fast-downloads',
    source: 'hls',
  },
];

/** The core's answer for a question that is no longer open. */
const ALREADY_ANSWERED =
  'That download was already confirmed or dismissed, or the browser took it back.';

/** The core's answer for a quality that needs the media add-on while it is missing. */
export const ADDON_REQUIRED =
  'This needs the media add-on, which joins video and audio and reads video pages. Install it from Settings, Add-ons (it is offered right away), then try again.';

/** How long the mock spends joining video and audio after a media download. */
const PROCESSING_MS = 2500;

export interface MockOptions {
  /** Total downloads. Beyond the hand-written samples, generated ones fill the list. */
  count?: number;
  /** How many generated downloads are running at once (for load testing the list). */
  active?: number;
  /** Start with no downloads at all, as on a first run. */
  empty?: boolean;
  /** Advance the simulation on a timer. Tests turn this off and call `tick` themselves. */
  autoTick?: boolean;
  tickMs?: number;
  /** Simulate one browser capture a few seconds after start. */
  autoCapture?: boolean;
  seed?: number;
  now?: () => number;
}

interface Sim {
  rate: number;
  /** Time spent connecting so far, in milliseconds. */
  connectingMs: number;
  /** Time spent joining video and audio so far, in milliseconds. */
  processingMs: number;
  /** Media that needs joining once downloaded. */
  needsJoin: boolean;
}

/** Small deterministic random source, so tests and screenshots are repeatable. */
function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const RUNNING = new Set(['connecting', 'downloading']);

function sameWait(a: WaitReason | null, b: WaitReason | null): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export class MockBackend implements Backend {
  readonly kind = 'mock';
  readonly sampleData: SampleData = {
    commands: [
      {
        id: 'mock.capture',
        title: 'Pretend the browser sent a download',
        keywords: 'simulate capture toast',
        run: () => this.simulateCapture(),
      },
      {
        id: 'mock.confirm',
        title: 'Pretend the browser sent a download that asks first',
        keywords: 'simulate confirmation',
        run: () => this.simulateConfirmation(),
      },
      {
        id: 'mock.media',
        title: 'Pretend the browser found a video with several qualities',
        keywords: 'simulate media quality picker stream youtube',
        run: () => this.simulateMediaChoice(),
      },
      {
        id: 'mock.addon',
        title: 'Pretend a site needs the media add-on',
        keywords: 'simulate addon add-on required youtube yt-dlp',
        run: () => this.simulateAddonRequired(),
      },
      {
        id: 'mock.elsewhere',
        title: 'Pretend the oldest question was answered somewhere else',
        keywords: 'simulate confirmation closed withdrawn another window',
        run: () => this.simulateAnsweredElsewhere(),
      },
      {
        id: 'mock.reading',
        title: 'Pretend the browser sent a video page that takes a while to read',
        keywords: 'simulate reading analysing page media',
        run: () => this.simulateSlowReading(),
      },
    ],
    now: () => this.#now(),
  };

  readonly #rows = new Map<DownloadId, DownloadSummary>();
  readonly #sim = new Map<DownloadId, Sim>();
  #order: DownloadId[] = [];
  #queues: Queue[] = defaultQueues();
  #settings: AppSettings = defaultSettings();
  #pending: PendingCapture[] = [];
  #mediaChoices: MediaChoice[] = [];
  #addons: AddonInfo[] = [mediaAddon()];
  /** Pages that asked for the media add-on; they continue once it is installed. */
  #waitingForAddon: Array<{ id: number; url: string }> = [];
  #mediaIndex = 0;
  readonly #listeners = new Set<(e: UiEvent) => void>();
  readonly #files = new Map<string, SettingsProfile>();
  #nextId = 1;
  #nextQueueId = 4;
  /** Confirmations and media choices share one id space, as in the core. */
  #nextPendingId = 1;
  #timer: ReturnType<typeof setInterval> | undefined;
  #captureTimer: ReturnType<typeof setTimeout> | undefined;
  readonly #random: () => number;
  readonly #now: () => number;
  readonly #tickMs: number;
  #captureIndex = 0;
  #lastTick: number;

  constructor(options: MockOptions = {}) {
    this.#random = rng(options.seed ?? 7);
    this.#now = options.now ?? (() => Date.now());
    this.#tickMs = options.tickMs ?? 250;
    this.#lastTick = this.#now();
    if (!options.empty) this.#seed(options.count ?? SEEDS.length, options.active ?? 0);
    // Work out what runs and why the rest waits before anyone asks.
    this.tick(0);
    if (options.autoTick ?? true) {
      this.#timer = setInterval(() => this.tick(), this.#tickMs);
    }
    if (options.autoCapture) {
      this.#captureTimer = setTimeout(() => this.simulateCapture(), 6000);
    }
  }

  /** Stops the timers. */
  dispose(): void {
    if (this.#timer) clearInterval(this.#timer);
    if (this.#captureTimer) clearTimeout(this.#captureTimer);
  }

  #seed(count: number, active: number) {
    const now = this.#now();
    for (const s of SEEDS.slice(0, count)) {
      const category = categoryFor(s.name, this.#settings.categories);
      const done = s.total === null ? 0 : Math.round(s.total * s.fraction);
      this.#insert(
        {
          id: this.#nextId++,
          url: s.url,
          file_name: s.name,
          path: `${folderFor(category, this.#settings)}\\${s.name}`,
          queue: s.queue,
          category,
          status: s.status,
          total: s.total === null ? null : Math.round(s.total),
          done,
          speed: 0,
          connections: 0,
          eta_secs: null,
          error: s.error ?? null,
          waiting: null,
          media: null,
          added_at: Math.round(now - s.age * 3_600_000),
          completed_at:
            s.status === 'completed' ? Math.round(now - (s.age - 0.2) * 3_600_000) : null,
        },
        s.rate,
      );
    }
    // Bulk rows for load testing: mostly finished, some waiting, `active` running.
    for (let i = SEEDS.length; i < count; i++) {
      const { name, url } = bulkItem(i);
      const category = categoryFor(name, this.#settings.categories);
      const total = Math.round((0.5 + this.#random() * 800) * 1024 * 1024);
      const isActive = i - SEEDS.length < active;
      const status = isActive ? 'downloading' : i % 9 === 0 ? 'queued' : 'completed';
      this.#insert(
        {
          id: this.#nextId++,
          url,
          file_name: name,
          path: `${folderFor(category, this.#settings)}\\${name}`,
          queue: isActive ? 1 : (i % 3) + 1,
          category,
          status,
          total,
          done: status === 'completed' ? total : Math.round(total * this.#random() * 0.5),
          speed: 0,
          connections: 0,
          eta_secs: null,
          error: null,
          waiting: null,
          media: null,
          added_at: now - i * 60_000,
          completed_at: status === 'completed' ? now - i * 60_000 + 30_000 : null,
        },
        (0.3 + this.#random() * 6) * 1024 * 1024,
      );
    }
    if (active > 0) {
      // Let the load test run every active row at once.
      this.#settings.max_concurrent_downloads = Math.max(4, active + 8);
      const main = this.#queues.find((q) => q.is_default);
      if (main) main.max_concurrent = Math.max(main.max_concurrent, active + 4);
    }
  }

  #insert(row: DownloadSummary, rate: number) {
    this.#rows.set(row.id, row);
    this.#sim.set(row.id, { rate, connectingMs: 0, processingMs: 0, needsJoin: false });
    this.#order.push(row.id);
  }

  #emit(event: UiEvent) {
    for (const l of this.#listeners) l(event);
  }

  #changed(rows: DownloadSummary[]) {
    if (rows.length > 0)
      this.#emit({ type: 'downloads_changed', data: rows.map((r) => ({ ...r })) });
  }

  #row(id: DownloadId): DownloadSummary {
    const row = this.#rows.get(id);
    if (!row) throw new BackendError('That download is no longer in the list.');
    return row;
  }

  #queue(id: QueueId): Queue {
    const q = this.#queues.find((x) => x.id === id);
    if (!q) throw new BackendError('That queue no longer exists.');
    return q;
  }

  #stop(row: DownloadSummary) {
    row.speed = 0;
    row.eta_secs = null;
    row.connections = 0;
  }

  /** Advances the simulation. Called on a timer, or directly by tests. */
  tick(elapsedMs?: number): void {
    const now = this.#now();
    const dt = elapsedMs ?? Math.max(1, now - this.#lastTick);
    this.#lastTick = now;
    const changed = new Map<DownloadId, DownloadSummary>();
    const date = new Date(now);

    // Decide what runs: each queue in order, up to its own limit and the global limit, and say
    // why everything else waits.
    const globalMax = this.#settings.max_concurrent_downloads;
    let globalRoom = globalMax;
    const runningByQueue = new Map<QueueId, DownloadSummary[]>();
    let queuesChanged = false;
    const setWait = (row: DownloadSummary, waiting: WaitReason | null) => {
      if (!sameWait(row.waiting, waiting)) {
        row.waiting = waiting;
        changed.set(row.id, row);
      }
    };
    for (const q of this.#queues) {
      const inWindow = q.running_outside_schedule || scheduleAllows(q.schedule, date);
      let room = !q.paused && inWindow ? q.max_concurrent : 0;
      const list: DownloadSummary[] = [];
      let waitingCount = 0;
      let active = 0;
      for (const id of this.#order) {
        const row = this.#rows.get(id);
        if (!row || row.queue !== q.id) continue;
        if (row.status !== 'queued' && !RUNNING.has(row.status)) {
          if (row.waiting !== null) setWait(row, null);
          continue;
        }
        active += 1;
        if (room > 0 && globalRoom > 0) {
          room -= 1;
          globalRoom -= 1;
          list.push(row);
          setWait(row, null);
          if (row.status === 'queued') {
            row.status = 'connecting';
            const sim = this.#sim.get(row.id);
            if (sim) sim.connectingMs = 0;
            changed.set(row.id, row);
          }
          continue;
        }
        if (row.status !== 'queued') {
          row.status = 'queued';
          this.#stop(row);
          changed.set(row.id, row);
        }
        waitingCount += 1;
        let why: WaitReason;
        if (q.paused) why = { type: 'queue_paused' };
        else if (!inWindow)
          why = { type: 'queue_schedule', data: { starts_at: q.schedule?.start ?? null } };
        else if (waitingCount > 1) why = { type: 'in_line', data: { position: waitingCount } };
        else if (room <= 0)
          why = {
            type: 'queue_full',
            data: { running: q.max_concurrent, limit: q.max_concurrent },
          };
        else why = { type: 'global_limit', data: { running: globalMax, limit: globalMax } };
        setWait(row, why);
      }
      // "Start now" lasts until the queue runs out of work.
      if (q.running_outside_schedule && active === 0) {
        q.running_outside_schedule = false;
        queuesChanged = true;
      }
      runningByQueue.set(q.id, list);
    }
    if (queuesChanged) this.#queuesChanged();

    // Move bytes, honoring queue limits and then the global limit.
    const wanted = new Map<DownloadId, number>();
    for (const q of this.#queues) {
      const list = runningByQueue.get(q.id) ?? [];
      const downloading = list.filter((r) => r.status === 'downloading');
      const share =
        q.speed_limit === null ? Infinity : q.speed_limit / Math.max(1, downloading.length);
      for (const row of list) {
        const sim = this.#sim.get(row.id);
        if (!sim) continue;
        if (row.status === 'connecting') {
          sim.connectingMs += dt;
          if (sim.connectingMs >= 700) {
            row.status = 'downloading';
            changed.set(row.id, row);
          }
          continue;
        }
        const wobble = 0.75 + this.#random() * 0.5;
        wanted.set(row.id, Math.min(share, sim.rate * wobble));
      }
    }
    let total = 0;
    for (const v of wanted.values()) total += v;
    const limit = this.#settings.global_speed_limit;
    const scale = limit !== null && total > limit ? limit / total : 1;

    for (const [id, want] of wanted) {
      const row = this.#rows.get(id);
      if (!row) continue;
      const speed = Math.round(want * scale);
      // Smooth like the core does, so numbers do not jump around.
      row.speed = row.speed === 0 ? speed : Math.round(row.speed * 0.6 + speed * 0.4);
      row.connections = Math.min(
        this.#settings.connections_per_download,
        row.total === null ? 1 : 8,
      );
      row.done += Math.round((speed * dt) / 1000);
      if (row.total !== null && row.done >= row.total) {
        row.done = row.total;
        // Media with separate video and audio is joined before it counts as finished.
        if (this.#sim.get(id)?.needsJoin) {
          row.status = 'processing';
        } else {
          row.status = 'completed';
          row.completed_at = now;
        }
        this.#stop(row);
      } else {
        row.eta_secs =
          row.total === null || row.speed === 0
            ? null
            : Math.round((row.total - row.done) / row.speed);
      }
      changed.set(id, row);
    }

    // Joining video and audio takes a moment, then the file is finished.
    for (const [id, sim] of this.#sim) {
      const row = this.#rows.get(id);
      if (row?.status !== 'processing' || wanted.has(id)) continue;
      sim.processingMs += dt;
      if (sim.processingMs >= PROCESSING_MS) {
        row.status = 'completed';
        row.completed_at = now;
        changed.set(id, row);
      }
    }
    this.#changed([...changed.values()]);
    this.#advanceInstall(dt);
  }

  // ---- Downloads ----

  async listDownloads() {
    return this.#order.flatMap((id) => {
      const r = this.#rows.get(id);
      return r ? [{ ...r }] : [];
    });
  }

  #create(
    url: string,
    opts: {
      queue?: QueueId | null;
      folder?: string | null;
      fileName?: string | null;
      start: boolean;
      size?: number | null;
    },
  ): DownloadSummary {
    const name = opts.fileName?.trim() || fileNameFromUrl(url);
    const category = categoryFor(name, this.#settings.categories);
    const folder = opts.folder?.trim() || folderFor(category, this.#settings);
    const queue = opts.queue ?? this.#queues.find((q) => q.is_default)?.id ?? 1;
    this.#queue(queue);
    const row: DownloadSummary = {
      id: this.#nextId++,
      url,
      file_name: name,
      path: `${folder}\\${name}`,
      queue,
      category,
      status: opts.start ? 'queued' : 'paused',
      total: opts.size ?? Math.round((2 + this.#random() * 900) * 1024 * 1024),
      done: 0,
      speed: 0,
      connections: 0,
      eta_secs: null,
      error: null,
      waiting: null,
      media: null,
      added_at: this.#now(),
      completed_at: null,
    };
    this.#insert(row, (1 + this.#random() * 12) * 1024 * 1024);
    return row;
  }

  async addDownload(request: AddRequest) {
    try {
      new URL(request.url);
    } catch {
      throw new BackendError(
        'That does not look like a link. Copy the full address, starting with https://.',
      );
    }
    const row = this.#create(request.url, {
      queue: request.queue,
      folder: request.folder,
      fileName: request.file_name,
      start: request.start,
    });
    this.#changed([row]);
    return row.id;
  }

  async pause(id: DownloadId) {
    const row = this.#row(id);
    if (row.status === 'completed' || row.status === 'paused') return;
    row.status = 'paused';
    row.waiting = null;
    this.#stop(row);
    this.#changed([row]);
  }

  async resume(id: DownloadId) {
    const row = this.#row(id);
    if (row.status !== 'paused' && row.status !== 'failed') return;
    row.status = 'queued';
    row.error = null;
    this.#changed([row]);
  }

  async cancel(id: DownloadId) {
    const row = this.#row(id);
    if (row.status === 'completed' || row.status === 'cancelled') return;
    row.status = 'cancelled';
    row.done = 0;
    row.error = null;
    this.#stop(row);
    this.#changed([row]);
  }

  async remove(ids: DownloadId[], _deleteFile: boolean) {
    const gone = new Set(ids.filter((id) => this.#rows.has(id)));
    for (const id of gone) {
      this.#rows.delete(id);
      this.#sim.delete(id);
    }
    this.#order = this.#order.filter((id) => !gone.has(id));
    if (gone.size > 0) this.#emit({ type: 'downloads_removed', data: [...gone] });
  }

  async retry(id: DownloadId) {
    const row = this.#row(id);
    // A cancelled download lost its partial file, so it starts from zero.
    if (row.status === 'cancelled') row.done = 0;
    row.status = 'queued';
    row.error = null;
    this.#changed([row]);
  }

  async refreshLink(id: DownloadId, url: string) {
    try {
      new URL(url);
    } catch {
      throw new BackendError(
        'That does not look like a link. Copy the full address, starting with https://.',
      );
    }
    const row = this.#row(id);
    row.url = url;
    row.status = 'queued';
    row.error = null;
    this.#changed([row]);
  }

  async moveToQueue(ids: DownloadId[], queue: QueueId) {
    this.#queue(queue);
    const rows = ids.flatMap((id) => {
      const r = this.#rows.get(id);
      if (!r || r.queue === queue) return [];
      r.queue = queue;
      return [r];
    });
    this.#changed(rows);
  }

  async reorder(queue: QueueId, idsInOrder: DownloadId[]) {
    const inQueue = this.#order.filter((id) => this.#rows.get(id)?.queue === queue);
    const wanted = idsInOrder.filter((id) => inQueue.includes(id));
    const rest = inQueue.filter((id) => !wanted.includes(id));
    const next = [...wanted, ...rest];
    let i = 0;
    this.#order = this.#order.map((id) =>
      this.#rows.get(id)?.queue === queue ? (next[i++] ?? id) : id,
    );
  }

  async pauseAll() {
    const rows: DownloadSummary[] = [];
    for (const row of this.#rows.values()) {
      if (row.status === 'queued' || RUNNING.has(row.status)) {
        row.status = 'paused';
        this.#stop(row);
        rows.push(row);
      }
    }
    this.#changed(rows);
  }

  async resumeAll() {
    const rows: DownloadSummary[] = [];
    for (const row of this.#rows.values()) {
      if (row.status === 'paused') {
        row.status = 'queued';
        rows.push(row);
      }
    }
    this.#changed(rows);
  }

  // ---- Queues ----

  async listQueues() {
    return this.#queues.map((q) => structuredClone(q));
  }

  #queuesChanged() {
    this.#emit({ type: 'queues_changed', data: this.#queues.map((q) => structuredClone(q)) });
  }

  async createQueue(name: string) {
    const trimmed = name.trim();
    if (!trimmed) throw new BackendError('Give the queue a name.');
    const q: Queue = {
      id: this.#nextQueueId++,
      name: trimmed,
      max_concurrent: 2,
      speed_limit: null,
      schedule: null,
      is_default: false,
      paused: false,
      running_outside_schedule: false,
    };
    this.#queues.push(q);
    this.#queuesChanged();
    return structuredClone(q);
  }

  async updateQueue(queue: Queue) {
    const i = this.#queues.findIndex((q) => q.id === queue.id);
    if (i < 0) throw new BackendError('That queue no longer exists.');
    if (!queue.name.trim()) throw new BackendError('Give the queue a name.');
    const old = this.#queues[i];
    // Pausing and "start now" have their own commands; an edit keeps the queue's current state.
    this.#queues[i] = {
      ...structuredClone(queue),
      is_default: old?.is_default ?? false,
      paused: old?.paused ?? false,
      running_outside_schedule: old?.running_outside_schedule ?? false,
    };
    this.#queuesChanged();
  }

  async setQueuePaused(id: QueueId, paused: boolean) {
    const q = this.#queue(id);
    q.paused = paused;
    if (paused) q.running_outside_schedule = false;
    this.#queuesChanged();
    this.tick(0);
  }

  async startQueueNow(id: QueueId) {
    const q = this.#queue(id);
    q.paused = false;
    if (!scheduleAllows(q.schedule, new Date(this.#now()))) q.running_outside_schedule = true;
    this.#queuesChanged();
    this.tick(0);
  }

  async deleteQueue(id: QueueId) {
    const q = this.#queue(id);
    if (q.is_default)
      throw new BackendError('The default queue cannot be deleted. Rename it instead.');
    const fallback = this.#queues.find((x) => x.is_default)?.id ?? 1;
    this.#queues = this.#queues.filter((x) => x.id !== id);
    const moved: DownloadSummary[] = [];
    for (const row of this.#rows.values()) {
      if (row.queue === id) {
        row.queue = fallback;
        moved.push(row);
      }
    }
    if (this.#settings.extension.default_queue === id) {
      this.#settings.extension.default_queue = fallback;
      this.#emit({ type: 'settings_changed', data: structuredClone(this.#settings) });
    }
    this.#queuesChanged();
    this.#changed(moved);
  }

  // ---- Settings ----

  async getSettings() {
    return structuredClone(this.#settings);
  }

  async updateSettings(settings: AppSettings) {
    this.#settings = structuredClone(settings);
    this.#emit({ type: 'settings_changed', data: structuredClone(this.#settings) });
  }

  async exportSettings(path: string) {
    this.#files.set(path, {
      format_version: 1,
      app_version: '0.1.0',
      exported_at: this.#now(),
      settings: structuredClone(this.#settings),
      queues: structuredClone(this.#queues),
      addons: this.#addons.filter((a) => a.state.type !== 'not_installed').map((a) => a.id),
    });
  }

  async importSettings(path: string) {
    const profile = this.#files.get(path);
    if (!profile) {
      throw new BackendError(
        'That file is not a Wirecatch settings file. Choose a file made with Export settings.',
      );
    }
    const known = new Set(this.#queues.map((q) => q.id));
    const added = profile.queues.filter((q) => !known.has(q.id)).length;
    this.#settings = structuredClone(profile.settings);
    this.#queues = structuredClone(profile.queues);
    this.#emit({ type: 'settings_changed', data: structuredClone(this.#settings) });
    this.#queuesChanged();
    const folder = profile.settings.default_folder;
    this.#emit({
      type: 'settings_imported',
      data: {
        folders: [{ setting: 'Default folder', requested: folder, used: folder, resolved: folder }],
        queues_added: added,
        queues_updated: profile.queues.length - added,
        // Like the core: offered, never installed by the import itself.
        addons_to_offer: profile.addons.filter(
          (id) => this.#addons.find((a) => a.id === id)?.state.type === 'not_installed',
        ),
      },
    });
  }

  // ---- Browser captures ----

  async pendingConfirmations() {
    return this.#pending.map((p) => ({ ...p }));
  }

  async confirmCapture(id: number, confirmation: CaptureConfirmation) {
    const p = this.#pending.find((x) => x.id === id);
    if (!p) throw new BackendError(ALREADY_ANSWERED);
    this.#pending = this.#pending.filter((x) => x.id !== id);
    this.#emit({ type: 'confirmation_closed', data: { id } });
    const row = this.#create(p.url, {
      queue: confirmation.queue ?? p.queue,
      folder: confirmation.folder ?? p.folder,
      fileName: confirmation.file_name ?? p.file_name,
      start: true,
      size: p.size,
    });
    this.#changed([row]);
  }

  async rejectCapture(id: number) {
    if (!this.#pending.some((x) => x.id === id)) throw new BackendError(ALREADY_ANSWERED);
    this.#pending = this.#pending.filter((x) => x.id !== id);
    this.#emit({ type: 'confirmation_closed', data: { id } });
  }

  /**
   * Sample data only: the oldest open question is answered somewhere else (another window, or
   * the browser took the download back), so the core closes it.
   */
  simulateAnsweredElsewhere(): void {
    const question = this.#pending[0] ?? this.#mediaChoices[0] ?? this.#waitingForAddon[0];
    if (!question) return;
    this.#pending = this.#pending.filter((x) => x.id !== question.id);
    this.#mediaChoices = this.#mediaChoices.filter((x) => x.id !== question.id);
    this.#waitingForAddon = this.#waitingForAddon.filter((x) => x.id !== question.id);
    this.#emit({ type: 'confirmation_closed', data: { id: question.id } });
  }

  /** Sample data only: pretends the browser extension handed over a download. */
  simulateCapture(): void {
    const samples = [
      ['https://github.com/example/app/releases/download/v3.2.0/app-setup-3.2.0.exe', 88],
      ['https://cdn.example-video.com/tutorials/color-grading-basics.mp4', 640],
      ['https://papers.example.org/2026/attention-at-scale.pdf', 3],
      ['https://downloads.example.com/drivers/gpu-driver-581.42.exe', 712],
    ] as const;
    const [url, sizeMb] = samples[this.#captureIndex++ % samples.length] ?? samples[0];
    const size = sizeMb * 1024 * 1024;
    const queue = this.#settings.extension.default_queue;
    if (this.#settings.extension.ask_before_download) {
      const name = fileNameFromUrl(url);
      const category = categoryFor(name, this.#settings.categories);
      const pending: PendingCapture = {
        id: this.#nextPendingId++,
        url,
        file_name: name,
        folder: folderFor(category, this.#settings),
        category,
        size,
        queue,
        page_url: `https://${new URL(url).host}/`,
      };
      this.#pending.push(pending);
      this.#emit({ type: 'confirmation_requested', data: { ...pending } });
      return;
    }
    const row = this.#create(url, { queue, start: true, size });
    this.#changed([row]);
    this.#emit({ type: 'captured', data: { ...row } });
  }

  /** Sample data only: a capture that asks first, whatever the setting says. */
  simulateConfirmation(): void {
    const ask = this.#settings.extension.ask_before_download;
    this.#settings.extension.ask_before_download = true;
    this.simulateCapture();
    this.#settings.extension.ask_before_download = ask;
  }

  // ---- Media and add-ons ----

  /** Sample data only: the browser found media with several qualities. */
  simulateMediaChoice(page?: string, id?: number): void {
    const sample = MEDIA_SAMPLES[this.#mediaIndex++ % MEDIA_SAMPLES.length] ?? MEDIA_SAMPLES[0];
    if (!sample) return;
    const MB = 1024 * 1024;
    const choice: MediaChoice = {
      id: id ?? this.#nextPendingId++,
      title: sample.title,
      page_url: page ?? sample.page,
      options: [
        {
          id: '2160',
          label: '4K (2160p), MP4',
          height: 2160,
          audio_only: false,
          size: 1840 * MB,
          needs_ffmpeg: true,
        },
        {
          id: '1080',
          label: '1080p, MP4',
          height: 1080,
          audio_only: false,
          size: 612 * MB,
          needs_ffmpeg: true,
        },
        {
          id: '720',
          label: '720p, MP4',
          height: 720,
          audio_only: false,
          size: 298 * MB,
          needs_ffmpeg: false,
        },
        {
          id: 'audio',
          label: 'Audio only, M4A',
          height: null,
          audio_only: true,
          size: null,
          needs_ffmpeg: false,
        },
      ],
      folder: folderFor('video', this.#settings),
      queue: this.#settings.extension.default_queue,
    };
    // Like the core: the reading is announced, then the choice comes under the same id.
    this.#emit({ type: 'media_analysing', data: { id: choice.id, url: choice.page_url ?? '' } });
    this.#mediaChoices.push(choice);
    this.#emit({ type: 'media_choice_requested', data: structuredClone(choice) });
  }

  /** Sample data only: a page that takes three seconds to read before its qualities show. */
  simulateSlowReading(): void {
    const id = this.#nextPendingId++;
    const page = 'https://video.example.com/watch/slow-page';
    this.#emit({ type: 'media_analysing', data: { id, url: page } });
    setTimeout(() => this.simulateMediaChoice(page, id), 3000);
  }

  /** Sample data only: a page that needs the media add-on to download. */
  simulateAddonRequired(): void {
    const page = 'https://www.youtube.com/watch?v=sample';
    const addon = this.#addon('media');
    if (addon.state.type === 'installed' || addon.state.type === 'update_available') {
      this.simulateMediaChoice(page);
      return;
    }
    const id = this.#nextPendingId++;
    this.#waitingForAddon.push({ id, url: page });
    this.#emit({ type: 'addon_required', data: { addon: structuredClone(addon), url: page, id } });
  }

  async pendingMediaChoices() {
    return structuredClone(this.#mediaChoices);
  }

  async pendingAddonRequests(): Promise<AddonRequestItem[]> {
    const addon = this.#addon('media');
    return this.#waitingForAddon.map((w) => ({ ...w, addon: structuredClone(addon) }));
  }

  async chooseMedia(id: number, selection: MediaSelection) {
    const choice = this.#mediaChoices.find((c) => c.id === id);
    if (!choice) throw new BackendError(ALREADY_ANSWERED);
    const option = choice.options.find((o) => o.id === selection.option_id);
    if (!option) {
      throw new BackendError(
        'That quality is not offered for this video. Choose one of the listed options.',
      );
    }
    const addon = this.#addon('media');
    const ready = addon.state.type === 'installed' || addon.state.type === 'update_available';
    if (option.needs_ffmpeg && !ready) {
      // Like the core: ask for the add-on and keep the choice open.
      const url = choice.page_url ?? choice.title;
      this.#emit({ type: 'addon_required', data: { addon: structuredClone(addon), url, id } });
      throw new BackendError(ADDON_REQUIRED);
    }
    this.#mediaChoices = this.#mediaChoices.filter((c) => c.id !== id);
    this.#emit({ type: 'confirmation_closed', data: { id } });
    const ext = option.audio_only ? 'm4a' : 'mp4';
    const name = selection.file_name?.trim() || `${choice.title}.${ext}`;
    const row = this.#create(choice.page_url ?? choice.title, {
      queue: selection.queue ?? choice.queue,
      folder: selection.folder ?? choice.folder,
      fileName: name,
      start: true,
      size: option.size ?? 48 * 1024 * 1024,
    });
    row.media = {
      quality: option.label.split(',')[0] ?? option.label,
      source: MEDIA_SAMPLES.find((s) => s.title === choice.title)?.source ?? 'page',
    };
    const sim = this.#sim.get(row.id);
    if (sim) {
      sim.needsJoin = option.needs_ffmpeg;
      // Sample media finishes in about eight seconds, so the joining step is quick to see.
      sim.rate = (row.total ?? 48 * 1024 * 1024) / 8;
    }
    this.#changed([row]);
  }

  async dismissMediaChoice(id: number) {
    // Like the core, this drops a quality choice or a page still waiting for the add-on.
    const known =
      this.#mediaChoices.some((c) => c.id === id) || this.#waitingForAddon.some((w) => w.id === id);
    if (!known) throw new BackendError(ALREADY_ANSWERED);
    this.#mediaChoices = this.#mediaChoices.filter((c) => c.id !== id);
    this.#waitingForAddon = this.#waitingForAddon.filter((w) => w.id !== id);
    this.#emit({ type: 'confirmation_closed', data: { id } });
  }

  #addon(id: string): AddonInfo {
    const addon = this.#addons.find((a) => a.id === id);
    if (!addon) throw new BackendError('That add-on is not available.');
    return addon;
  }

  #addonsChanged() {
    this.#emit({ type: 'addons_changed', data: structuredClone(this.#addons) });
  }

  async listAddons() {
    return structuredClone(this.#addons);
  }

  async installAddon(id: string) {
    const addon = this.#addon(id);
    if (addon.state.type === 'installing') return;
    this.#beforeInstall.set(id, structuredClone(addon.state));
    addon.state = { type: 'installing', data: { done: 0, total: addon.size } };
    this.#addonsChanged();
  }

  /** What each add-on was before its install or update started, for Stop. */
  readonly #beforeInstall = new Map<string, AddonInfo['state']>();

  async updateAddon(id: string) {
    await this.installAddon(id);
  }

  /** Set by tests and sample data: what the next update check finds. */
  nextUpdateCheck: 'up-to-date' | 'newer' | 'unreachable' = 'up-to-date';

  async checkAddonUpdates() {
    const addon = this.#addon('media');
    const installed = addon.state.type === 'installed' || addon.state.type === 'update_available';
    if (installed && this.nextUpdateCheck === 'unreachable') {
      throw new BackendError(
        'Wirecatch could not check for a newer media add-on (no connection). Check your internet connection, then try again.',
      );
    }
    if (installed && this.nextUpdateCheck === 'newer') {
      addon.state = { type: 'update_available' };
      addon.latest_version = 'yt-dlp 2026.10.30, ffmpeg 9.0.2, Deno 2.9.7';
      this.#addonsChanged();
    }
    return structuredClone(this.#addons);
  }

  async cancelAddonOperation(id: string) {
    const addon = this.#addon(id);
    if (addon.state.type !== 'installing') return;
    // Like the core: an update keeps the installed version; a first install leaves nothing.
    const before = this.#beforeInstall.get(id);
    const kept = before?.type === 'installed' || before?.type === 'update_available';
    addon.state = kept && before ? before : { type: 'not_installed' };
    if (!kept) addon.installed_version = null;
    this.#addonsChanged();
  }

  async removeAddon(id: string) {
    const addon = this.#addon(id);
    addon.state = { type: 'not_installed' };
    addon.installed_version = null;
    this.#addonsChanged();
  }

  /** Moves a simulated add-on install along, about four seconds from start to finish. */
  #advanceInstall(dt: number) {
    let changed = false;
    for (const addon of this.#addons) {
      if (addon.state.type !== 'installing') continue;
      const total = addon.state.data.total ?? 96 * 1024 * 1024;
      const done = Math.min(total, addon.state.data.done + Math.round((total * dt) / 4000));
      changed = true;
      if (done < total) {
        addon.state = { type: 'installing', data: { done, total } };
        continue;
      }
      addon.state = { type: 'installed' };
      addon.installed_version = addon.latest_version;
      // What asked for the add-on continues on its own, under the same id.
      const waiting = this.#waitingForAddon;
      this.#waitingForAddon = [];
      queueMicrotask(() => {
        for (const page of waiting) this.simulateMediaChoice(page.url, page.id);
      });
    }
    if (changed) this.#addonsChanged();
  }

  async takeLaunchLinks(): Promise<string[]> {
    return [];
  }

  async onShellSignal(_listener: (signal: ShellSignal) => void): Promise<Unsubscribe> {
    // A browser tab has no desktop shell to signal anything.
    return () => {};
  }

  // ---- Files and pickers ----

  /** Files opened, for tests: the mock opens nothing for real. */
  readonly opened: DownloadId[] = [];

  async openFile(id: DownloadId, allowProgram = false) {
    const row = this.#row(id);
    if (row.status !== 'completed') {
      throw new BackendError('The file is not finished yet. It opens once the download completes.');
    }
    if (isProgramName(row.file_name) && !allowProgram) {
      throw new BackendError(
        `${row.file_name} is a type of file that can run code. Only open it if you trust ${hostOf(row.url)}. Use Show in folder to open it yourself.`,
      );
    }
    this.opened.push(id);
  }

  async isProgram(id: DownloadId) {
    return isProgramName(this.#row(id).file_name);
  }

  async openFolder(id: DownloadId) {
    this.#row(id);
  }

  async setDownloadFolder(id: DownloadId, folder: string) {
    const row = this.#row(id);
    row.path = `${folder}\\${row.file_name}`;
    if (row.error?.fix === 'choose_folder') {
      row.error = null;
      row.status = 'queued';
    }
    this.#changed([row]);
  }

  async subscribe(listener: (event: UiEvent) => void): Promise<Unsubscribe> {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  #folderPick = 0;
  async pickFolder(current?: string) {
    const options = [`${HOME}\\Videos`, `${DOWNLOADS}\\Later`, `D:\\Media`, `${HOME}\\Desktop`];
    const pick = options[this.#folderPick++ % options.length] ?? DOWNLOADS;
    return pick === current ? (options[this.#folderPick++ % options.length] ?? DOWNLOADS) : pick;
  }

  async pickSaveFile(suggestedName: string) {
    return `${HOME}\\Documents\\${suggestedName}`;
  }

  async pickOpenFile() {
    const last = [...this.#files.keys()].pop();
    return last ?? `${HOME}\\Documents\\wirecatch-settings.json`;
  }

  /** Test helper: the folder a row will land in. */
  folderOf(id: DownloadId): string {
    return folderOf(this.#row(id).path);
  }
}
