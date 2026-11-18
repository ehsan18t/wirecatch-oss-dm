/**
 * In-memory stand-ins for the browser and the Wirecatch app, so the extension's
 * flows run end to end in tests without a browser, a bridge or an app.
 */
import {
  DEFAULT_MEDIA_TABLES,
  downloadId,
  MIN_PROTOCOL_VERSION,
  PROTOCOL_VERSION,
  queueId,
  type AddAction,
  type Call,
  type Event,
  type CheckAnswer,
  type OptionList,
  type PageOptions,
  type ShellConfig,
  type Hello,
  type Message,
  type Outcome,
  type ProtocolError,
  type Reply,
  type QueueSummary,
  type WithdrawOutcome,
} from '@wirecatch/protocol';
import { DEFAULT_OPTIONS } from '@wirecatch/protocol/testing/options';
import { vi } from 'vitest';
import { browser } from 'wxt/browser';
import { fakeBrowser } from 'wxt/testing/fake-browser';
import type { DownloadItemLike } from '../src/lib/capture/candidate';
import type { CookieQuery, CookieStore, StoredCookie } from '../src/lib/cookies';
import type { DownloadState } from '../src/lib/capture/handover';
import type {
  BrowserDownloads,
  InFlightEntry,
  InFlightStore,
  AlertStore,
} from '../src/lib/hub/deps';
import type { CachedSettings, SettingsCache } from '../src/lib/cache';
import type { Channel, Alert } from '../src/lib/popup-port';
import type { NativePort, NativeTransport } from '../src/lib/native/client';

/** The values the extension's checks use, as a fresh install has them (D279). */
export const DEFAULT_SETTINGS: ShellConfig = {
  revision: { instance: 'fake', counter: 1 },
  capture_enabled: true,
  private_windows: true,
  show_media_button: true,
  ignored_sites: [],
  capture_extensions: [],
  min_capture_size: 0,
  video_sites: [],
  media: DEFAULT_MEDIA_TABLES,
};

/** `config` under the next revision, as the app sends a change. */
export function nextRevision(config: ShellConfig): ShellConfig {
  return {
    ...config,
    revision: { ...config.revision, counter: config.revision.counter + 1 },
  };
}

/** `list` with option `id` holding `value`, as the app sends it after a change. */
export function withOptionValue(list: OptionList, id: string, value: unknown): OptionList {
  return {
    revision: { ...list.revision, counter: list.revision.counter + 1 },
    groups: list.groups.map((group) => ({
      ...group,
      options: group.options.map((option) =>
        typeof option === 'object' && option !== null && 'id' in option && option.id === id
          ? { ...option, value }
          : option,
      ),
    })),
  };
}

/** How the fake app answers `check_download`. */
export type CheckBehavior = 'take' | 'hang' | { leave: string } | { cross_site: boolean };

/** The site of an address with its scheme, without a list of public suffixes: enough for the fake's hosts. */
function fakeSite(address: string | null | undefined): string | null {
  if (address === null || address === undefined) return null;
  try {
    const url = new URL(address);
    return `${url.protocol}//${url.hostname.split('.').slice(-2).join('.')}`;
  } catch {
    return null;
  }
}

export const QUEUES: QueueSummary[] = [
  { id: queueId(1), name: 'Main', is_default: true },
  { id: queueId(2), name: 'Night', is_default: false },
];

/** Resets the fake browser and has `cookies.getAll` answer with `answer`; a throw rejects the query. */
export function stubGetAll(answer: (query: CookieQuery) => StoredCookie[]): void {
  fakeBrowser.reset();
  vi.spyOn(browser.cookies, 'getAll').mockImplementation(
    // The fake's typings take the callback form; the extension uses the promise form.
    (query) => new Promise((resolve) => resolve(answer(query as CookieQuery))) as never,
  );
}

/** The default cookie store: a normal window's in Chromium, which names no store. */
export const NORMAL_STORE: CookieStore = { id: null, private: false };

/** Chromium's cookie stores, each with the tabs it holds, and whether each tab is private. */
export function stubCookieStores(
  stores: { id: string; tabIds: number[] }[],
  tabs: { id: number; incognito: boolean }[],
): void {
  vi.spyOn(browser.cookies, 'getAllCookieStores').mockResolvedValue(stores as never);
  vi.spyOn(browser.tabs, 'query').mockResolvedValue(tabs as never);
}

/** Lets pending promise callbacks run (they are queued as microtasks). */
export async function flush(): Promise<void> {
  for (let i = 0; i < 10; i += 1) await Promise.resolve();
}

type Listener<T> = (value: T) => void;

export class FakeNativePort implements NativePort {
  closed = false;
  /** True when the extension closed this port itself (`port.disconnect()`). */
  closedByExtension = false;
  /** Methods received on this port, in order. */
  readonly methods: Call['method'][] = [];
  reason: string | null = null;
  readonly #messageListeners: Listener<unknown>[] = [];
  readonly #disconnectListeners: (() => void)[] = [];

  constructor(private readonly app: FakeApp) {}

  onMessage = {
    addListener: (listener: Listener<unknown>) => void this.#messageListeners.push(listener),
  };
  onDisconnect = {
    addListener: (listener: () => void) => void this.#disconnectListeners.push(listener),
  };

  postMessage(message: Message): void {
    if (this.closed) throw new Error('Attempting to use a disconnected port object');
    if (message.kind === 'request') this.methods.push(message.call.method);
    this.app.receive(this, message);
  }

  disconnect(): void {
    this.closed = true;
    this.closedByExtension = true;
  }

  /** Delivers a frame from the bridge, asynchronously like a real port. */
  deliver(message: Message): void {
    void Promise.resolve().then(() => {
      if (this.closed) return;
      for (const listener of this.#messageListeners) listener(message);
    });
  }

  /** The browser closes the port (host missing, bridge exited, ...). */
  browserDisconnect(reason: string | null): void {
    if (this.closed) return;
    this.closed = true;
    this.reason = reason;
    for (const listener of this.#disconnectListeners) listener();
  }
}

export type AddBehavior =
  AddAction | { error: ProtocolError } | 'hang' | 'disconnect' | 'unexpected_action';

/** Like the bridge: a hello without `start_app` comes from an older client, which always started the app. */
function mayStartApp(hello: Partial<Pick<Hello, 'start_app'>>): boolean {
  return hello.start_app ?? true;
}

/** The bridge plus the app, answering the protocol like the real ones would. */
export class FakeApp implements NativeTransport {
  settings: ShellConfig = { ...DEFAULT_SETTINGS };
  options: OptionList = DEFAULT_OPTIONS;
  queues: QueueSummary[] = QUEUES;
  checkBehavior: CheckBehavior = 'take';
  /** The protocol versions the app speaks. */
  versions = { oldest: MIN_PROTOCOL_VERSION, newest: PROTOCOL_VERSION };
  /** Whether the app reads `max_protocol_version`; an app from before version ranges reads only `protocol_version`. */
  readsVersionRanges = true;
  /** When set, `hello` is answered with this version whatever the client speaks, as a broken app might. */
  chosenVersion: number | null = null;
  /** Calls answered with `unsupported_call`, as an app that does not have them answers. */
  readonly unsupported = new Set<Call['method']>();
  appVersion = '0.1.0';
  /** When set, every new connection fails like a missing native host. */
  hostMissing: string | null = null;
  /** When set, `hello` is answered with this error (for example `app_unavailable`). */
  helloError: ProtocolError | null = null;
  /** Whether the app is running. A `hello` that allows it starts a closed app; one that does not is answered `app_not_running`, as the bridge does. */
  running = true;
  /** How many times a `hello` started the app. */
  starts = 0;
  /** When set, a `hello` that starts the closed app is answered only after this many milliseconds, as the app takes to start; until then the app is not running. */
  startDelayMs: number | null = null;
  /** When false, nothing is answered (a hung bridge). */
  answers = true;
  addBehavior: AddBehavior = 'started';
  /** When set, `add_download` is answered (as `addBehavior` says) only after this many milliseconds. */
  addLateMs: number | null = null;
  /** `unreadable`: answered with an outcome this protocol version does not have. */
  withdrawBehavior: WithdrawOutcome | 'hang' | 'unreadable' | { error: ProtocolError } =
    'withdrawn';
  /** How to answer the withdraw of one handover, over `withdrawBehavior`. */
  readonly withdrawFor = new Map<string, FakeApp['withdrawBehavior']>();
  /** When set, `set_option` and `invoke_action` are refused with this error. */
  updateError: ProtocolError | null = null;
  /** Every request received, in order. */
  readonly requests: { id: number; call: Call }[] = [];
  readonly ports: FakeNativePort[] = [];
  /** Called when a request arrives, before it is answered. */
  onRequest: ((call: Call) => void) | null = null;

  connect(): NativePort {
    const port = new FakeNativePort(this);
    this.ports.push(port);
    if (this.hostMissing !== null) {
      const reason = this.hostMissing;
      void Promise.resolve().then(() => port.browserDisconnect(reason));
    }
    return port;
  }

  disconnectReason(port: NativePort): string | null {
    return (port as FakeNativePort).reason;
  }

  get openPort(): FakeNativePort | undefined {
    return this.ports.findLast((p) => !p.closed);
  }

  calls(method: Call['method']): Call[] {
    return this.requests.filter((r) => r.call.method === method).map((r) => r.call);
  }

  /** The app changes its settings on its own (for example in its settings window). */
  changeSettings(changes: Partial<ShellConfig>): void {
    this.settings = nextRevision({ ...this.settings, ...changes });
    this.broadcast({ type: 'shell_config_changed', data: this.settings });
    this.options = { ...this.syncedOptions(), revision: this.settings.revision };
    this.broadcast({ type: 'options_changed', data: this.options });
  }

  /** The app changes one of the popup's options, as in its settings window. */
  changeOption(id: string, value: unknown): void {
    this.options = withOptionValue(this.options, id, value);
    this.broadcast({ type: 'options_changed', data: this.options });
  }

  broadcast(event: Event): void {
    for (const port of this.ports) if (!port.closed) port.deliver({ kind: 'event', event });
  }

  receive(port: FakeNativePort, message: Message): void {
    if (message.kind !== 'request') return;
    const { id, call } = message;
    this.requests.push({ id, call });
    this.onRequest?.(call);
    if (this.answers) this.answer(port, id, call);
  }

  /** Answers one request the way the app would, as the behavior fields say. */
  private answer(port: FakeNativePort, id: number, call: Call): void {
    const reply = (outcome: Outcome): void => port.deliver({ kind: 'response', id, outcome });
    if (this.unsupported.has(call.method)) {
      reply({ status: 'err', value: { code: 'unsupported_call', message: 'Not in this app.' } });
      return;
    }
    const answered = this.plainReply(call.method);
    if (answered !== null) {
      reply({ status: 'ok', value: answered });
      return;
    }
    if (call.method === 'hello') this.greet(call.params, reply);
    else if (call.method === 'set_option')
      this.setOption(call.params.id, call.params.change, reply);
    else if (call.method === 'invoke_action') this.act(call.params.id, call.params.site, reply);
    else if (call.method === 'page_options') this.page(call.params.host, reply);
    else if (call.method === 'check_download') this.check(call.params.url, call.params, reply);
    else if (call.method === 'withdraw_download') this.withdraw(call.params.handover_id, reply);
    else if (call.method === 'add_download') this.add(port, { id, call }, reply);
  }

  /** The answer to a call that only reads, or only asks for something to be shown; null for any other call. */
  private plainReply(method: Call['method']): Reply | null {
    const done: Reply = { type: 'done' };
    const replies: Partial<Record<Call['method'], Reply>> = {
      get_shell_config: { type: 'shell_config', data: this.settings },
      get_options: { type: 'options', data: this.syncedOptions() },
      list_queues: { type: 'queues', data: this.queues },
      private_session_ended: done,
      pause_all: done,
      resume_all: done,
      show_window: done,
      show_browser_settings: done,
    };
    return replies[method] ?? null;
  }

  /** Answers `hello`, after `startDelayMs` when it starts the app. */
  private greet(params: Hello, reply: (outcome: Outcome) => void): void {
    const delay = this.startDelayMs;
    if (!this.running && mayStartApp(params) && delay !== null) {
      setTimeout(() => reply(this.hello(params)), delay);
      return;
    }
    reply(this.hello(params));
  }

  private hello(params: Hello): Outcome {
    if (!this.running && mayStartApp(params)) {
      this.running = true;
      this.starts += 1;
    }
    if (!this.running) {
      return {
        status: 'err',
        value: { code: 'app_not_running', message: 'Wirecatch is not running.' },
      };
    }
    if (this.helloError !== null) return { status: 'err', value: this.helloError };
    const version = this.chosenVersion ?? this.agreedVersion(params);
    if (version === null) {
      return {
        status: 'err',
        value: { code: 'unsupported_version', message: 'No protocol version in common.' },
      };
    }
    return {
      status: 'ok',
      value: { type: 'hello', data: { app_version: this.appVersion, protocol_version: version } },
    };
  }

  /** The newest version both the app and the client speak, as the app chooses it; null when they share none. */
  private agreedVersion(params: Hello): number | null {
    const oldest = params.protocol_version;
    const newest = this.readsVersionRanges ? (params.max_protocol_version ?? oldest) : oldest;
    const agreed = Math.min(newest, this.versions.newest);
    return agreed >= Math.max(oldest, this.versions.oldest) ? agreed : null;
  }

  /** The options with the values `settings` holds for the ones they share, as the app keeps them alike. */
  syncedOptions(): OptionList {
    let list = this.options;
    for (const [id, value] of Object.entries(this.settings)) {
      if (id !== 'revision')
        list = { ...withOptionValue(list, id, value), revision: list.revision };
    }
    return list;
  }

  private act(id: string, site: string | null, reply: (outcome: Outcome) => void): void {
    if (this.updateError === null && site !== null) {
      const covers = (entry: string): boolean => site === entry || site.endsWith(`.${entry}`);
      const sites = this.settings.ignored_sites;
      if (id === 'ignore_site' && !sites.includes(site)) {
        this.changeSettings({ ignored_sites: [...sites, site.replace(/^www\./, '')] });
      } else if (id === 'take_site') {
        this.changeSettings({ ignored_sites: sites.filter((entry) => !covers(entry)) });
      }
    }
    this.refusedOr(reply, { type: 'done' });
  }

  private refusedOr(reply: (outcome: Outcome) => void, value: Reply): void {
    if (this.updateError === null) {
      reply({ status: 'ok', value });
    } else {
      reply({ status: 'err', value: this.updateError });
    }
  }

  private setOption(
    id: string,
    change: Extract<Call, { method: 'set_option' }>['params']['change'],
    reply: (outcome: Outcome) => void,
  ): void {
    if (this.updateError === null && 'set' in change) {
      this.options = withOptionValue(this.options, id, change.set);
      if (id in this.settings) {
        this.settings = nextRevision({ ...this.settings, [id]: change.set });
        this.broadcast({ type: 'shell_config_changed', data: this.settings });
      }
    }
    this.refusedOr(reply, { type: 'options', data: this.syncedOptions() });
    if (this.updateError === null) {
      this.broadcast({ type: 'options_changed', data: this.syncedOptions() });
    }
  }

  private page(host: string, reply: (outcome: Outcome) => void): void {
    const entry = this.settings.ignored_sites.find(
      (site) => host === site || host.endsWith(`.${site}`),
    );
    const page: PageOptions = {
      site: entry ?? host.replace(/^www\./, ''),
      ignored: entry !== undefined,
    };
    reply({ status: 'ok', value: { type: 'page_options', data: page } });
  }

  private check(
    url: string,
    params: Extract<Call, { method: 'check_download' }>['params'],
    reply: (outcome: Outcome) => void,
  ): void {
    const behavior = this.checkBehavior;
    if (behavior === 'hang') return;
    const request = params.cookie_request;
    const site = fakeSite(url);
    const answer: CheckAnswer =
      typeof behavior === 'object' && 'leave' in behavior
        ? { decision: 'leave', reason: behavior.leave }
        : {
            decision: 'take',
            cookies: {
              site,
              top_level_site: request.navigation ? site : fakeSite(request.top_level),
              // Another site than the page that asked for it, or than the tab's page, as the app judges it.
              cross_site:
                typeof behavior === 'object'
                  ? behavior.cross_site
                  : [request.initiator, request.top_level].some(
                      (page) => page !== null && page !== undefined && fakeSite(page) !== site,
                    ),
            },
          };
    reply({ status: 'ok', value: { type: 'download_check', data: answer } });
  }

  private withdraw(handoverId: string, reply: (outcome: Outcome) => void): void {
    const behavior = this.withdrawFor.get(handoverId) ?? this.withdrawBehavior;
    if (behavior === 'hang') return;
    if (typeof behavior === 'object') reply({ status: 'err', value: behavior.error });
    else if (behavior === 'unreadable') {
      reply({ status: 'ok', value: { type: 'withdrawn', data: 'vanished' as WithdrawOutcome } });
    } else reply({ status: 'ok', value: { type: 'withdrawn', data: behavior } });
  }

  /** Answers `add_download` as `addBehavior` and `addLateMs` say. */
  private add(
    port: FakeNativePort,
    request: { id: number; call: Call },
    reply: (outcome: Outcome) => void,
  ): void {
    const behavior = this.addBehavior;
    if (behavior === 'hang') return;
    if (this.addLateMs !== null) {
      const late = this.addLateMs;
      this.addLateMs = null;
      setTimeout(() => this.receiveAgain(port, request.id, request.call), late);
      return;
    }
    if (behavior === 'disconnect') {
      void Promise.resolve().then(() => port.browserDisconnect('Native host has exited.'));
      return;
    }
    if (behavior === 'unexpected_action') {
      reply({
        status: 'ok',
        value: { type: 'download_added', data: { id: null, action: 'exploded' as AddAction } },
      });
      return;
    }
    if (typeof behavior === 'object') {
      reply({ status: 'err', value: behavior.error });
      return;
    }
    reply({
      status: 'ok',
      value: {
        type: 'download_added',
        data: { id: behavior === 'awaiting_confirmation' ? null : downloadId(7), action: behavior },
      },
    });
  }

  /** Answers a request that was held back (used by `addLateMs`), without recording it twice. */
  private receiveAgain(port: FakeNativePort, id: number, call: Call): void {
    const before = this.requests.length;
    const hook = this.onRequest;
    this.onRequest = null;
    this.receive(port, { kind: 'request', id, call });
    this.onRequest = hook;
    this.requests.splice(before);
  }
}

/** A browser downloads API that records every call in order and can be told to fail. */
export class FakeDownloads implements BrowserDownloads {
  readonly log: string[] = [];
  readonly failing = new Set<'pause' | 'resume' | 'cancel' | 'erase' | 'start' | 'state'>();
  /** Calls that fail for one download only, written as in `log` (`resume:5`). */
  readonly failingCalls = new Set<string>();
  /** What `state()` reports (the browser's own view of the download). */
  browserState: DownloadState | null = { state: 'in_progress', paused: true };
  /** What `state()` reports for one download, over `browserState`. */
  readonly states = new Map<number, DownloadState | null>();

  /** Runs after each logged call succeeds, as the browser would change the download. */
  afterCall: ((call: string) => void) | null = null;

  state(id: number): Promise<DownloadState | null> {
    if (this.failing.has('state')) return Promise.reject(new Error('search failed'));
    return Promise.resolve(this.states.has(id) ? (this.states.get(id) ?? null) : this.browserState);
  }

  #run(op: 'pause' | 'resume' | 'cancel' | 'erase' | 'start', arg: number | string): Promise<void> {
    const call = `${op}:${String(arg)}`;
    this.log.push(call);
    if (this.failing.has(op) || this.failingCalls.has(call)) {
      return Promise.reject(new Error(`${op} failed`));
    }
    if (op === 'cancel' && typeof arg === 'number') this.#cancelled(arg);
    if ((op === 'pause' || op === 'resume') && typeof arg === 'number') {
      this.#setPaused(arg, op === 'pause');
    }
    this.afterCall?.(call);
    return Promise.resolve();
  }

  /** As the browser does: a cancel interrupts a download in progress, and does nothing to a finished one. */
  #cancelled(id: number): void {
    const state = this.states.has(id) ? this.states.get(id) : this.browserState;
    if (state?.state === 'in_progress')
      this.states.set(id, { state: 'interrupted', paused: false });
  }

  /** As the browser does: pausing or resuming changes only a download in progress. */
  #setPaused(id: number, paused: boolean): void {
    if (this.states.has(id)) {
      const state = this.states.get(id);
      if (state?.state === 'in_progress') this.states.set(id, { ...state, paused });
    } else if (this.browserState?.state === 'in_progress') {
      this.browserState = { ...this.browserState, paused };
    }
  }

  pause(id: number): Promise<void> {
    return this.#run('pause', id);
  }
  resume(id: number): Promise<void> {
    return this.#run('resume', id);
  }
  cancel(id: number): Promise<void> {
    return this.#run('cancel', id);
  }
  erase(id: number): Promise<void> {
    return this.#run('erase', id);
  }
  start(url: string): Promise<void> {
    return this.#run('start', url);
  }
}

export class MemoryInFlight implements InFlightStore {
  entries: InFlightEntry[] = [];
  /** Every list saved, in order. */
  readonly saves: InFlightEntry[][] = [];
  load(): Promise<InFlightEntry[]> {
    return Promise.resolve([...this.entries]);
  }
  save(entries: InFlightEntry[]): Promise<void> {
    this.entries = [...entries];
    this.saves.push([...entries]);
    return Promise.resolve();
  }
}

export class MemoryAlert implements AlertStore {
  value: Alert[] = [];
  #gate: Promise<void> = Promise.resolve();

  /** Makes `load` answer only once the returned function is called, as a slow storage read does. */
  hold(): () => void {
    let release: () => void = () => undefined;
    this.#gate = new Promise((resolve) => {
      release = resolve;
    });
    return release;
  }

  /** Reads the value when called, like storage does, and answers once any hold is released. */
  async load(): Promise<Alert[]> {
    const value = this.value;
    await this.#gate;
    return value;
  }
  save(alerts: Alert[]): Promise<void> {
    this.value = [...alerts];
    return Promise.resolve();
  }
}

export class MemoryCache implements SettingsCache {
  value: CachedSettings | null = null;
  load(): Promise<CachedSettings | null> {
    return Promise.resolve(this.value);
  }
  save(value: CachedSettings): Promise<void> {
    this.value = value;
    return Promise.resolve();
  }
}

/** Two connected ends of a `runtime.Port`, like the popup and the background have. */
export function channelPair<A, B>(): [Channel<A, B>, Channel<B, A>, () => void] {
  const aListeners: ((m: B) => void)[] = [];
  const bListeners: ((m: A) => void)[] = [];
  const aDisconnect: (() => void)[] = [];
  const bDisconnect: (() => void)[] = [];
  let open = true;
  const a: Channel<A, B> = {
    postMessage: (m) => {
      if (!open) throw new Error('Attempting to use a disconnected port object');
      const copy = structuredClone(m);
      void Promise.resolve().then(() => bListeners.forEach((l) => l(copy)));
    },
    onMessage: { addListener: (l) => void aListeners.push(l) },
    onDisconnect: { addListener: (l) => void aDisconnect.push(l) },
  };
  const b: Channel<B, A> = {
    postMessage: (m) => {
      if (!open) throw new Error('Attempting to use a disconnected port object');
      const copy = structuredClone(m);
      void Promise.resolve().then(() => aListeners.forEach((l) => l(copy)));
    },
    onMessage: { addListener: (l) => void bListeners.push(l) },
    onDisconnect: { addListener: (l) => void bDisconnect.push(l) },
  };
  const close = (): void => {
    open = false;
    aDisconnect.forEach((l) => l());
    bDisconnect.forEach((l) => l());
  };
  return [a, b, close];
}

export function downloadItem(overrides: Partial<DownloadItemLike> = {}): DownloadItemLike {
  return {
    id: 1,
    url: 'https://files.example.com/big.zip',
    finalUrl: 'https://files.example.com/big.zip',
    referrer: 'https://example.com/downloads',
    filename: 'big.zip',
    mime: 'application/zip',
    totalBytes: 50_000_000,
    fileSize: 50_000_000,
    incognito: false,
    state: 'in_progress',
    ...overrides,
  };
}
