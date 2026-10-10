// An in-memory stand-in for the core, so the interface can be built, reviewed and tested in a
// normal browser. It simulates queues, limits, schedules, varied speeds, failures with fixes,
// and browser captures, and pushes the same `UiEvent`s the real core will.

import type {
  AddRequest,
  AppSettings,
  CaptureConfirmation,
  DownloadId,
  DownloadSummary,
  PendingCapture,
  Queue,
  QueueId,
  SettingsProfile,
  UiEvent,
} from '@wirecatch/protocol';
import { fileNameFromUrl, folderOf, scheduleAllows } from '../format';
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
import { BackendError, type Backend, type Unsubscribe } from './types';

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

export class MockBackend implements Backend {
  readonly kind = 'mock';

  readonly #rows = new Map<DownloadId, DownloadSummary>();
  readonly #sim = new Map<DownloadId, Sim>();
  #order: DownloadId[] = [];
  #queues: Queue[] = defaultQueues();
  #settings: AppSettings = defaultSettings();
  #pending: PendingCapture[] = [];
  readonly #listeners = new Set<(e: UiEvent) => void>();
  readonly #files = new Map<string, SettingsProfile>();
  #nextId = 1;
  #nextQueueId = 4;
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
    this.#sim.set(row.id, { rate, connectingMs: 0 });
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

    // Decide what runs: each queue in order, up to its own limit and the global limit.
    let globalRoom = this.#settings.max_concurrent_downloads;
    const runningByQueue = new Map<QueueId, DownloadSummary[]>();
    for (const q of this.#queues) {
      let room = scheduleAllows(q.schedule, date) ? q.max_concurrent : 0;
      const list: DownloadSummary[] = [];
      for (const id of this.#order) {
        const row = this.#rows.get(id);
        if (!row || row.queue !== q.id) continue;
        if (row.status !== 'queued' && !RUNNING.has(row.status)) continue;
        if (room > 0 && globalRoom > 0) {
          room -= 1;
          globalRoom -= 1;
          list.push(row);
          if (row.status === 'queued') {
            row.status = 'connecting';
            const sim = this.#sim.get(row.id);
            if (sim) sim.connectingMs = 0;
            changed.set(row.id, row);
          }
        } else if (row.status !== 'queued') {
          row.status = 'queued';
          this.#stop(row);
          changed.set(row.id, row);
        }
      }
      runningByQueue.set(q.id, list);
    }

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
        row.status = 'completed';
        row.completed_at = now;
        this.#stop(row);
      } else {
        row.eta_secs =
          row.total === null || row.speed === 0
            ? null
            : Math.round((row.total - row.done) / row.speed);
      }
      changed.set(id, row);
    }
    this.#changed([...changed.values()]);
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
    this.#queues[i] = { ...structuredClone(queue), is_default: old?.is_default ?? false };
    this.#queuesChanged();
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
      addons: [],
    });
  }

  async importSettings(path: string) {
    const profile = this.#files.get(path);
    if (!profile) {
      throw new BackendError(
        'That file is not a Wirecatch settings file. Choose a file made with Export settings.',
      );
    }
    this.#settings = structuredClone(profile.settings);
    this.#queues = structuredClone(profile.queues);
    this.#emit({ type: 'settings_changed', data: structuredClone(this.#settings) });
    this.#queuesChanged();
  }

  // ---- Browser captures ----

  async pendingConfirmations() {
    return this.#pending.map((p) => ({ ...p }));
  }

  async confirmCapture(id: number, confirmation: CaptureConfirmation) {
    const p = this.#pending.find((x) => x.id === id);
    if (!p) throw new BackendError('That download was already answered.');
    this.#pending = this.#pending.filter((x) => x.id !== id);
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
    this.#pending = this.#pending.filter((x) => x.id !== id);
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

  // ---- Files and pickers ----

  async openFile(id: DownloadId) {
    const row = this.#row(id);
    if (row.status !== 'completed') {
      throw new BackendError('The file is not finished yet. It opens once the download completes.');
    }
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
