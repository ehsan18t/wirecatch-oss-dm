// What the mock core holds, and the small operations every part of the mock shares: looking
// things up, adding a download, and telling listeners what changed.

import {
  type AddonInfo,
  type AppSettings,
  type DownloadId,
  type DownloadSummary,
  type MediaChoice,
  type MediaReading,
  type PendingCapture,
  type ProcessingStep,
  type ProxyServer,
  type ProxyServerId,
  type Queue,
  type QuestionId,
  type QueueId,
  type SettingsFile,
  type SuffixListInfo,
  type UiEvent,
  type UserError,
  downloadId,
  proxyServerId,
  questionId,
  queueId,
  type QueueReason,
} from '@wirecatch/protocol';
import { MB } from '@wirecatch/units';
import { fileNameFromUrl } from '../../format';
import { proxyAddress } from '../../proxy';
import type { WindowRegistry } from '../../sample-windows';
import { BackendError } from '../types';
import {
  builtInSuffixList,
  categoryFor,
  defaultProxies,
  defaultQueues,
  defaultSettings,
  folderFor,
  mediaAddon,
  MAIN_QUEUE_ID,
} from './data';
import { between, type Random, type Spread } from './rng';

/** How a download moves along in the simulation. */
export interface Sim {
  /** How fast it can go, in bytes a second. */
  rate: number;
  /** Time spent connecting so far, in milliseconds. */
  connectingMs: number;
  /** Time spent joining or converting so far, in milliseconds. */
  processingMs: number;
  /** Media that needs joining once downloaded. */
  needsJoin: boolean;
  /** What ffmpeg does to it once downloaded (D24), if anything besides joining. */
  step: ProcessingStep | null;
}

/** A page that asked for the media add-on; it continues under the same id once it is installed. */
export interface AddonPage {
  id: QuestionId;
  url: string;
}

/** What a new download needs; everything else starts empty. */
export interface NewRow {
  url: string;
  file_name: string;
  queue: QueueId;
  /** By default the person's pick when the row names its queue in the app, else the default queue's. */
  queue_reason?: QueueReason;
  status: DownloadSummary['status'];
  total: number | null;
  done: number;
  added_at: number;
  /** The folder; by default the one its category goes to. */
  folder?: string;
  error?: UserError | null;
  completed_at?: number | null;
}

/** What `create` needs to add a download someone asked for. */
export interface CreateOptions {
  queue?: QueueId | null;
  queue_reason?: QueueReason;
  folder?: string | null;
  fileName?: string | null;
  start: boolean;
  size?: number | null;
}

/** Downloads added without a known size are this many megabytes, and go this fast in megabytes a second. */
const ADDED_SIZE_MB: Spread = { min: 2, span: 900 };
const ADDED_RATE_MB: Spread = { min: 1, span: 12 };

/** Statuses of a download that is running. */
export const RUNNING: ReadonlySet<DownloadSummary['status']> = new Set([
  'connecting',
  'downloading',
]);

/** Clears what only a running download has. */
export function stop(row: DownloadSummary): void {
  row.speed = 0;
  row.eta_secs = null;
  row.connections = 0;
}

/** `text` without surrounding spaces, or null when nothing is left. */
export function nonBlank(text: string | null | undefined): string | null {
  const trimmed = text?.trim();
  return trimmed === undefined || trimmed === '' ? null : trimmed;
}

export class MockState {
  readonly rows = new Map<DownloadId, DownloadSummary>();
  readonly sim = new Map<DownloadId, Sim>();
  order: DownloadId[] = [];
  queues: Queue[] = defaultQueues();
  settings: AppSettings = defaultSettings();
  pending: PendingCapture[] = [];
  mediaChoices: MediaChoice[] = [];
  addons: AddonInfo[] = [mediaAddon()];
  suffixList: SuffixListInfo = builtInSuffixList();
  proxyServers: ProxyServer[] = defaultProxies();
  /** Pages being read for their qualities. */
  reading: MediaReading[] = [];
  /** Pages that asked for the media add-on; they continue once it is installed. */
  waitingForAddon: AddonPage[] = [];
  /** Settings files, by path: the mock writes none for real. */
  readonly files = new Map<string, SettingsFile>();
  /** What each add-on was before its install or update started, for Stop. */
  readonly beforeInstall = new Map<string, AddonInfo['state']>();
  readonly listeners = new Set<(e: UiEvent) => void>();
  #nextDownload = 1;
  #nextQueue = Math.max(...this.queues.map((q) => q.id)) + 1;
  #nextProxy = Math.max(0, ...this.proxyServers.map((p) => p.id)) + 1;
  /** Confirmations and media choices share one id space, as in the core. */
  #nextQuestion = 1;

  constructor(
    readonly windows: WindowRegistry,
    readonly random: Random,
    readonly now: () => number,
  ) {}

  /** The id of the next download added. */
  newDownloadId(): DownloadId {
    return downloadId(this.#nextDownload++);
  }

  /** The id of the next queue made. */
  newQueueId(): QueueId {
    return queueId(this.#nextQueue++);
  }

  /** The id of the next proxy server saved. */
  newProxyId(): ProxyServerId {
    return proxyServerId(this.#nextProxy++);
  }

  /** The id of the next question asked: a file to confirm, a page being read or a quality to pick. */
  newQuestionId(): QuestionId {
    return questionId(this.#nextQuestion++);
  }

  emit(event: UiEvent): void {
    for (const l of this.listeners) l(event);
    // Like the desktop shell: every question opens its own window.
    if (
      event.type === 'confirmation_requested' ||
      event.type === 'media_choice_requested' ||
      event.type === 'media_analysing' ||
      event.type === 'addon_required'
    ) {
      this.windows.ask(event.data.id);
    } else if (event.type === 'confirmation_closed') {
      this.windows.closed(event.data.id);
    }
  }

  changed(rows: DownloadSummary[]): void {
    if (rows.length > 0)
      // Copies all the way down, as the real app's events are: a store must never hold an object this keeps changing.
      this.emit({ type: 'downloads_changed', data: rows.map((r) => structuredClone(r)) });
  }

  queuesChanged(): void {
    this.emit({ type: 'queues_changed', data: this.queues.map((q) => structuredClone(q)) });
  }

  proxyServersChanged(): void {
    this.emit({
      type: 'proxy_servers_changed',
      data: this.proxyServers.map((p) => structuredClone(p)),
    });
  }

  settingsChanged(): void {
    this.emit({ type: 'settings_changed', data: structuredClone(this.settings) });
  }

  addonsChanged(): void {
    this.emit({ type: 'addons_changed', data: structuredClone(this.addons) });
  }

  row(id: DownloadId): DownloadSummary {
    const row = this.rows.get(id);
    if (!row) throw new BackendError('That download is no longer in the list.');
    return row;
  }

  queue(id: QueueId): Queue {
    const q = this.queues.find((x) => x.id === id);
    if (!q) throw new BackendError('That queue no longer exists.');
    return q;
  }

  defaultQueueId(): QueueId {
    return this.queues.find((q) => q.is_default)?.id ?? MAIN_QUEUE_ID;
  }

  addon(id: string): AddonInfo {
    const addon = this.addons.find((a) => a.id === id);
    if (!addon) throw new BackendError('That add-on is not available.');
    return addon;
  }

  /** Whether the media add-on is installed, so media can be joined and converted. */
  mediaReady(): boolean {
    const { type } = this.addon('media').state;
    return type === 'installed' || type === 'update_available';
  }

  /** Every open question, oldest first, as the desktop lists them. */
  openQuestions(): QuestionId[] {
    const ids = [
      ...this.pending.map((p) => p.id),
      ...this.mediaChoices.map((c) => c.id),
      ...this.waitingForAddon.map((w) => w.id),
      ...this.reading.map((r) => r.id),
    ];
    return ids.filter((id, i) => ids.indexOf(id) === i).sort((a, b) => a - b);
  }

  /** The proxy a run in `queue` goes through, in the core's words. */
  proxyLabel(queue: QueueId): string {
    const own = this.queues.find((q) => q.id === queue)?.settings.proxy;
    const proxy = own?.source === 'own' ? own.value : this.settings.proxy.proxy;
    // The sample computer sets no proxy, so the computer's goes directly too.
    if (proxy.kind !== 'server') return 'No proxy';
    const profile = this.proxyServers.find((p) => p.id === proxy.id);
    if (!profile) return 'No proxy';
    const address = proxyAddress(profile.settings);
    const name = profile.settings.name.trim();
    return name === '' ? address : `${name} (${address})`;
  }

  /** Adds a download at the end of the list; `rate` is how fast it can go. */
  insert(fields: NewRow, rate: number): DownloadSummary {
    const category = categoryFor(fields.file_name, this.settings.categories);
    const folder = fields.folder ?? folderFor(category, this.settings);
    const row: DownloadSummary = {
      id: this.newDownloadId(),
      url: fields.url,
      file_name: fields.file_name,
      path: `${folder}\\${fields.file_name}`,
      queue: fields.queue,
      queue_reason: fields.queue_reason ?? { kind: 'default_queue' },
      category,
      status: fields.status,
      total: fields.total,
      done: fields.done,
      speed: 0,
      connections: 0,
      eta_secs: null,
      error: fields.error ?? null,
      waiting: null,
      media: null,
      processing: null,
      // The mock names the proxy when a download is added; the core, when each run starts.
      proxy:
        fields.status === 'queued' || fields.status === 'paused'
          ? null
          : this.proxyLabel(fields.queue),
      added_at: fields.added_at,
      completed_at: fields.completed_at ?? null,
    };
    this.rows.set(row.id, row);
    this.sim.set(row.id, { rate, connectingMs: 0, processingMs: 0, needsJoin: false, step: null });
    this.order.push(row.id);
    return row;
  }

  /** Adds a download someone asked for, in a queue that must exist. */
  create(url: string, opts: CreateOptions): DownloadSummary {
    const queue = opts.queue ?? this.defaultQueueId();
    this.queue(queue);
    const fields: NewRow = {
      url,
      file_name: nonBlank(opts.fileName) ?? fileNameFromUrl(url),
      queue,
      queue_reason: opts.queue_reason,
      status: opts.start ? 'queued' : 'paused',
      total: opts.size ?? Math.round(between(this.random, ADDED_SIZE_MB) * MB),
      done: 0,
      added_at: this.now(),
      folder: nonBlank(opts.folder) ?? undefined,
    };
    return this.insert(fields, between(this.random, ADDED_RATE_MB) * MB);
  }
}
