/**
 * In-memory stand-ins for the browser and the Wirecatch app, so the extension's
 * flows run end to end in tests without a browser, a bridge or an app.
 */
import type {
  AddAction,
  Call,
  Event,
  ExtensionSettings,
  ExtensionSettingsPatch,
  Hello,
  Message,
  Outcome,
  ProtocolError,
  QueueSummary,
  WithdrawOutcome,
} from '@wirecatch/protocol';
import type { DownloadItemLike } from '../src/lib/capture/candidate';
import type { DownloadState } from '../src/lib/capture/handover';
import type {
  BrowserDownloads,
  CachedSettings,
  InFlightStore,
  SettingsCache,
} from '../src/lib/hub/deps';
import type { Channel } from '../src/lib/messages';
import type { NativePort, NativeTransport } from '../src/lib/native/client';
import { applyPatch } from '../src/lib/protocol';

export const DEFAULT_SETTINGS: ExtensionSettings = {
  capture_enabled: true,
  ask_before_download: false,
  show_media_button: true,
  ignored_sites: [],
  capture_extensions: [],
  min_capture_size: 0,
  default_queue: 1,
};

export const QUEUES: QueueSummary[] = [
  { id: 1, name: 'Main', is_default: true },
  { id: 2, name: 'Night', is_default: false },
];

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

/** The bridge plus the app, answering the protocol like the real ones would. */
/** Like the bridge: a hello without `start_app` comes from an older client, which always started the app. */
function mayStartApp(hello: Partial<Pick<Hello, 'start_app'>>): boolean {
  return hello.start_app ?? true;
}

export class FakeApp implements NativeTransport {
  settings: ExtensionSettings = { ...DEFAULT_SETTINGS };
  queues: QueueSummary[] = QUEUES;
  protocolVersion = 1;
  appVersion = '0.1.0';
  /** When set, every new connection fails like a missing native host. */
  hostMissing: string | null = null;
  /** When set, `hello` is answered with this error (for example `app_unavailable`). */
  helloError: ProtocolError | null = null;
  /** Whether the app is running. A `hello` that allows it starts a closed app; one that does not is answered `app_not_running`, as the bridge does. */
  running = true;
  /** How many times a `hello` started the app. */
  starts = 0;
  /** When false, nothing is answered (a hung bridge). */
  answers = true;
  addBehavior: AddBehavior = 'started';
  /** When set, `add_download` is answered (as `addBehavior` says) only after this many milliseconds. */
  addLateMs: number | null = null;
  withdrawBehavior: WithdrawOutcome | 'hang' | { error: ProtocolError } = 'withdrawn';
  /** When set, `update_extension_settings` is refused with this error. */
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
    return this.ports.filter((p) => !p.closed).at(-1);
  }

  calls(method: Call['method']): Call[] {
    return this.requests.filter((r) => r.call.method === method).map((r) => r.call);
  }

  /** The app changes its settings on its own (for example in its settings window). */
  changeSettings(changes: Partial<ExtensionSettings>): void {
    this.settings = { ...this.settings, ...changes };
    this.broadcast({ type: 'extension_settings_changed', data: this.settings });
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
    switch (call.method) {
      case 'hello':
        reply(this.hello(call.params));
        return;
      case 'get_extension_settings':
        reply({ status: 'ok', value: { type: 'extension_settings', data: this.settings } });
        return;
      case 'update_extension_settings':
        this.updateSettings(call.params, reply);
        return;
      case 'list_queues':
        reply({ status: 'ok', value: { type: 'queues', data: this.queues } });
        return;
      case 'pause_all':
      case 'resume_all':
      case 'show_window':
        reply({ status: 'ok', value: { type: 'done' } });
        return;
      case 'withdraw_download':
        this.withdraw(reply);
        return;
      case 'add_download':
        this.add(port, { id, call }, reply);
        return;
    }
  }

  private hello(params: Hello): Outcome {
    if (!this.running && mayStartApp(params)) {
      this.running = true;
      this.starts += 1;
    }
    if (!this.running) {
      return { status: 'err', value: { code: 'app_not_running', message: 'Wirecatch is closed.' } };
    }
    if (this.helloError !== null) return { status: 'err', value: this.helloError };
    return {
      status: 'ok',
      value: {
        type: 'hello',
        data: { app_version: this.appVersion, protocol_version: this.protocolVersion },
      },
    };
  }

  private updateSettings(patch: ExtensionSettingsPatch, reply: (outcome: Outcome) => void): void {
    if (this.updateError !== null) {
      reply({ status: 'err', value: this.updateError });
      return;
    }
    this.settings = applyPatch(this.settings, patch);
    reply({ status: 'ok', value: { type: 'extension_settings', data: this.settings } });
    this.broadcast({ type: 'extension_settings_changed', data: this.settings });
  }

  private withdraw(reply: (outcome: Outcome) => void): void {
    const behavior = this.withdrawBehavior;
    if (behavior === 'hang') return;
    if (typeof behavior === 'object') reply({ status: 'err', value: behavior.error });
    else reply({ status: 'ok', value: { type: 'withdrawn', data: behavior } });
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
        data: { id: behavior === 'awaiting_confirmation' ? null : 7, action: behavior },
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
  readonly failing = new Set<'pause' | 'resume' | 'cancel' | 'erase' | 'start'>();
  /** What `state()` reports (the browser's own view of the download). */
  browserState: DownloadState | null = { state: 'in_progress', paused: true };

  state(): Promise<DownloadState | null> {
    return Promise.resolve(this.browserState);
  }

  #run(op: 'pause' | 'resume' | 'cancel' | 'erase' | 'start', arg: number | string): Promise<void> {
    this.log.push(`${op}:${String(arg)}`);
    if (this.failing.has(op)) return Promise.reject(new Error(`${op} failed`));
    return Promise.resolve();
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
  ids: number[] = [];
  /** Every list saved, in order. */
  readonly saves: number[][] = [];
  load(): Promise<number[]> {
    return Promise.resolve([...this.ids]);
  }
  save(ids: number[]): Promise<void> {
    this.ids = [...ids];
    this.saves.push([...ids]);
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
