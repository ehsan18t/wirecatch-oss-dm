import {
  MAX_FRAME_LEN,
  PROTOCOL_VERSION,
  UNKNOWN_REQUEST_ID,
  type Call,
  type Event,
  type Hello,
  type Message,
  type Outcome,
  type ProtocolError,
  type Reply,
} from '@wirecatch/protocol';
import { isEvent, isOutcome } from '../protocol';
import { isNumber, isUnknown, literal, object, oneOf } from '../validate';
import { formatBytes } from '@wirecatch/units';
import { classifyDisconnect, type ConnectionStatus, type Problem } from './status';

/** How many timed-out requests are remembered so a late answer can still be recognized. */
const LATE_MEMORY = 64;

/** The part of `runtime.Port` the client uses, so tests can pass a fake. */
export interface NativePort {
  postMessage(message: Message): void;
  disconnect(): void;
  onMessage: { addListener(listener: (message: unknown) => void): void };
  onDisconnect: { addListener(listener: () => void): void };
}

export interface NativeTransport {
  /** Opens a new connection to the native host (`runtime.connectNative`). Throws if the browser refuses. */
  connect(): NativePort;
  /** Why the port closed, read inside the disconnect listener (Chromium `runtime.lastError`, Firefox `port.error`). */
  disconnectReason(port: NativePort): string | null;
}

export interface ClientOptions {
  transport: NativeTransport;
  /** Builds the `hello` call sent first on every connection; the client adds whether the app may be started. */
  hello: () => Promise<Omit<Hello, 'start_app'>>;
  /** How long to wait for the answer to `hello`; `HELLO_TIMEOUT_MS` when missing. */
  helloTimeoutMs?: number;
  /** Delays between reconnection attempts; the last one repeats. */
  backoffMs?: Backoff;
  now?: () => number;
}

export type RequestErrorKind =
  /** No answer in time. */
  | 'timeout'
  /** No connection to the app, or it broke before the answer came. */
  | 'disconnected'
  /** The app answered with an error. */
  | 'app'
  /**
   * The app answered in a shape this extension cannot read (a newer app, or a broken one).
   * Whether it did what was asked is unknown, so callers treat it like no answer.
   */
  | 'unreadable'
  /** The request is larger than the bridge accepts; it was not sent. */
  | 'too_large';

export class RequestError extends Error {
  constructor(
    readonly kind: RequestErrorKind,
    message: string,
    readonly protocolError: ProtocolError | null = null,
  ) {
    super(message);
    this.name = 'RequestError';
  }
}

interface Pending {
  call: Call;
  resolve(reply: Reply): void;
  reject(error: RequestError): void;
  timer: ReturnType<typeof setTimeout>;
}

/** Delays between reconnection attempts, in milliseconds; never empty, so there is always one to repeat. */
export type Backoff = readonly [number, ...number[]];

/** Retries come quickly at first (the app may be starting) and then settle at one every 30 seconds. */
const DEFAULT_BACKOFF: Backoff = [1_000, 2_000, 4_000, 8_000, 15_000, 30_000];

/** How long the first answer, to `hello`, may take: the bridge may have to start the app first. */
const HELLO_TIMEOUT_MS = 10_000;

/**
 * The outer shape of a frame from the bridge: an answer to a request, or an event. Only
 * the envelope is checked here; what it carries is checked by `isOutcome` and `isEvent`,
 * so an answer that cannot be read still settles the request it belongs to.
 */
const isFrame = oneOf(
  object({
    kind: literal('response'),
    id: isNumber,
    outcome: isUnknown,
  }),
  object({ kind: literal('event'), event: isUnknown }),
);

/** An answer the app marked as an error, whatever else it carries. */
const isErrorAnswer = object({ status: literal('err') });

/** Why a request failed when the app's answer to it does not match the protocol. */
const UNREADABLE_ANSWER = 'Wirecatch gave an answer this extension does not understand.';

/** The delay before retry number `attempt` (counted from 0): past the end of the table, its last delay. */
export function backoffDelay(backoff: Backoff, attempt: number): number {
  const [first, ...rest] = backoff;
  return attempt === 0 ? first : (rest[Math.min(attempt, rest.length) - 1] ?? first);
}

const encoder = new TextEncoder();

function utf8Length(text: string): number {
  return encoder.encode(text).length;
}

/** The connection problem an error from the bridge stands for, or null when it is about one request only. The bridge answers every request on a port it could not connect with the same error, so such a port is done. */
function connectionProblem(error: ProtocolError): Problem | null {
  switch (error.code) {
    case 'app_not_running':
      return { kind: 'app_closed', detail: null };
    case 'app_unavailable':
      return { kind: 'app_unavailable', detail: error.message };
    case 'unsupported_version':
    case 'invalid_request':
    case 'invalid_url':
    case 'not_found':
    case 'internal':
    case 'not_started':
    case 'addon_required':
      return null;
  }
}

function problemFromHelloError(error: RequestError): Problem {
  const known = error.protocolError === null ? null : connectionProblem(error.protocolError);
  if (known !== null) return known;
  if (error.protocolError?.code === 'unsupported_version') {
    return { kind: 'version_mismatch', detail: error.message };
  }
  return { kind: 'disconnected', detail: error.message };
}

/** What is wrong with the answer to `hello`, or null when the app can be talked to. */
function helloProblem(reply: Reply): Problem | null {
  if (reply.type !== 'hello') {
    return { kind: 'disconnected', detail: 'Wirecatch gave an unexpected answer to hello.' };
  }
  if (reply.data.protocol_version !== PROTOCOL_VERSION) {
    return {
      kind: 'version_mismatch',
      detail: `The app speaks protocol version ${reply.data.protocol_version}; this extension speaks version ${PROTOCOL_VERSION}.`,
    };
  }
  return null;
}

/**
 * Talks to the Wirecatch app through the native messaging bridge.
 *
 * Sends `hello` first on every connection, matches answers to requests by id,
 * times requests out, passes app events on, and reconnects with backoff when the
 * connection breaks.
 *
 * Only a request, which the person's own action causes (a download from the browser, the media button, a popup action), or the popup's Open button may start the app. Connecting on start-up and reconnecting only reach an app that is already running, and once the app is known to be closed nothing is retried, so a Wirecatch the person closed stays closed until something needs it (D25).
 */
export class NativeClient {
  readonly #options: ClientOptions;
  readonly #backoff: Backoff;
  readonly #now: () => number;
  #status: ConnectionStatus = { state: 'connecting' };
  #port: NativePort | null = null;
  /** Bumped whenever a port is dropped, so late events from it are ignored. */
  #generation = 0;
  #nextId = UNKNOWN_REQUEST_ID + 1;
  #attempt = 0;
  /** Requests that timed out on the current port, so an answer arriving later is passed on. */
  readonly #late = new Map<number, Call>();
  readonly #lateListeners = new Set<(call: Call, outcome: Outcome | null) => void>();
  #started = false;
  #stopped = false;
  /** Whether the open attempt may start the app. */
  #attemptStartsApp = false;
  /** A request or the Open button came while an attempt that may not start the app was open: if it finds the app closed, the client starts it right after. */
  #startWanted = false;
  #retryTimer: ReturnType<typeof setTimeout> | null = null;
  readonly #pending = new Map<number, Pending>();
  readonly #readyWaiters = new Set<{ resolve(): void; reject(error: RequestError): void }>();
  readonly #statusListeners = new Set<(status: ConnectionStatus) => void>();
  readonly #eventListeners = new Set<(event: Event) => void>();

  constructor(options: ClientOptions) {
    this.#options = options;
    this.#backoff = options.backoffMs ?? DEFAULT_BACKOFF;
    this.#now = options.now ?? Date.now;
  }

  get status(): ConnectionStatus {
    return this.#status;
  }

  /**
   * Connects for the first time, to an app that is already running. Does nothing when a
   * connection exists or is being made (a request may have connected first, perhaps
   * starting the app; replacing that port would lose its answers), and nothing when
   * called again.
   */
  start(): void {
    if (this.#started) return;
    this.#started = true;
    const idle =
      !this.#stopped &&
      this.#port === null &&
      this.#retryTimer === null &&
      this.#status.state !== 'connected';
    if (idle) void this.#connect(false);
  }

  /** Closes the connection for good and fails everything still waiting. */
  stop(): void {
    this.#stopped = true;
    this.#clearRetry();
    this.#dropPort();
    this.#rejectAll(new RequestError('disconnected', 'The connection to Wirecatch was closed.'));
  }

  /** Skips the remaining backoff and connects right away, starting the app if it is closed (the popup's "Try again" and "Open Wirecatch", and every request). */
  reconnectNow(): void {
    if (this.#stopped || this.#status.state === 'connected') return;
    if (this.#isAttempting()) {
      if (!this.#attemptStartsApp) this.#startWanted = true;
      return;
    }
    this.#clearRetry();
    void this.#connect(true);
  }

  onStatus(listener: (status: ConnectionStatus) => void): () => void {
    this.#statusListeners.add(listener);
    return () => this.#statusListeners.delete(listener);
  }

  onEvent(listener: (event: Event) => void): () => void {
    this.#eventListeners.add(listener);
    return () => this.#eventListeners.delete(listener);
  }

  /** Called when the app answers a request after it already timed out here; `outcome` is null when the answer could not be read. */
  onLateAnswer(listener: (call: Call, outcome: Outcome | null) => void): () => void {
    this.#lateListeners.add(listener);
    return () => this.#lateListeners.delete(listener);
  }

  /**
   * Sends a call and resolves with the app's reply. Waits for the connection
   * (starting an attempt if none is running) within the same time limit.
   * Rejects with a `RequestError` on timeout, disconnection or an app error.
   */
  async request(call: Call, timeoutMs: number): Promise<Reply> {
    const deadline = this.#now() + timeoutMs;
    if (this.#status.state !== 'connected') {
      await this.#waitUntilConnected(timeoutMs);
    }
    return this.#send(call, Math.max(0, deadline - this.#now()));
  }

  #isAttempting(): boolean {
    return this.#port !== null && this.#status.state !== 'connected';
  }

  #waitUntilConnected(timeoutMs: number): Promise<void> {
    if (this.#stopped) {
      return Promise.reject(
        new RequestError('disconnected', 'The connection to Wirecatch was closed.'),
      );
    }
    return new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#readyWaiters.delete(waiter);
        reject(new RequestError('timeout', 'Wirecatch did not answer in time.'));
      }, timeoutMs);
      const waiter = {
        resolve: () => {
          clearTimeout(timer);
          resolve();
        },
        reject: (error: RequestError) => {
          clearTimeout(timer);
          reject(error);
        },
      };
      this.#readyWaiters.add(waiter);
      this.reconnectNow();
    });
  }

  /** Opens a new connection. With `startApp` off, the bridge only reaches an app that is running. */
  async #connect(startApp: boolean): Promise<void> {
    this.#dropPort();
    const generation = this.#generation;
    this.#attemptStartsApp = startApp;
    this.#startWanted = false;
    if (this.#status.state === 'unavailable') {
      this.#setStatus({ ...this.#status, retryAt: null });
    }
    if (this.#openPort(generation)) await this.#greet(startApp, generation);
  }

  /** Opens the port and listens to it for this connection only; false when the browser refuses. */
  #openPort(generation: number): boolean {
    let port: NativePort;
    try {
      port = this.#options.transport.connect();
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      this.#fail({ kind: classifyDisconnect(detail), detail });
      return false;
    }
    this.#port = port;
    port.onMessage.addListener((raw) => {
      if (generation === this.#generation) this.#onMessage(raw);
    });
    port.onDisconnect.addListener(() => {
      if (generation !== this.#generation) return;
      const reason = this.#options.transport.disconnectReason(port);
      this.#fail({ kind: classifyDisconnect(reason), detail: reason });
    });
    return true;
  }

  /** Says hello; the connection is up only once the app answers in a protocol version this extension speaks. */
  async #greet(startApp: boolean, generation: number): Promise<void> {
    let reply: Reply;
    try {
      const hello: Hello = { ...(await this.#options.hello()), start_app: startApp };
      if (generation !== this.#generation) return;
      reply = await this.#send(
        { method: 'hello', params: hello },
        this.#options.helloTimeoutMs ?? HELLO_TIMEOUT_MS,
      );
    } catch (error) {
      if (generation !== this.#generation) return;
      const failure =
        error instanceof RequestError ? error : new RequestError('disconnected', String(error));
      this.#fail(problemFromHelloError(failure));
      return;
    }
    if (generation !== this.#generation) return;
    const problem = helloProblem(reply);
    if (problem !== null || reply.type !== 'hello') {
      this.#fail(problem ?? { kind: 'disconnected', detail: null });
      return;
    }
    this.#attempt = 0;
    this.#setStatus({ state: 'connected', appVersion: reply.data.app_version });
    for (const waiter of this.#readyWaiters) waiter.resolve();
    this.#readyWaiters.clear();
  }

  #send(call: Call, timeoutMs: number): Promise<Reply> {
    const port = this.#port;
    if (port === null) {
      return Promise.reject(new RequestError('disconnected', 'Wirecatch is not connected.'));
    }
    const id = this.#nextId++;
    if (this.#nextId === UNKNOWN_REQUEST_ID) this.#nextId += 1;
    const message: Message = { kind: 'request', id, call };
    // Checked here so one oversized request fails on its own instead of making the bridge drop the port.
    // MAX_FRAME_LEN is the largest message the bridge accepts (D14), as `crates/protocol` sets it.
    if (utf8Length(JSON.stringify(message)) > MAX_FRAME_LEN) {
      return Promise.reject(
        new RequestError(
          'too_large',
          `This request is too large to send to Wirecatch (over ${formatBytes(MAX_FRAME_LEN)}). Try a shorter list or a shorter address.`,
        ),
      );
    }
    return new Promise<Reply>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        this.#rememberLate(id, call);
        reject(new RequestError('timeout', 'Wirecatch did not answer in time.'));
      }, timeoutMs);
      this.#pending.set(id, { call, resolve, reject, timer });
      try {
        port.postMessage(message);
      } catch (error) {
        clearTimeout(timer);
        this.#pending.delete(id);
        const detail = error instanceof Error ? error.message : String(error);
        reject(new RequestError('disconnected', `Could not send to Wirecatch: ${detail}`));
      }
    });
  }

  #onMessage(raw: unknown): void {
    if (!isFrame(raw)) return;
    if (raw.kind === 'event') {
      // Malformed, or an event this version does not know: nothing here acts on it.
      if (!isEvent(raw.event)) return;
      for (const listener of this.#eventListeners) listener(raw.event);
      return;
    }
    if (isOutcome(raw.outcome)) {
      this.#onResponse(raw.id, raw.outcome);
      return;
    }
    this.#onResponse(raw.id, null);
    // An error that cannot be read may be one that ends the port (app_not_running in a newer
    // shape): connect again with a fresh hello rather than keep a port the bridge gave up on.
    if (isErrorAnswer(raw.outcome)) this.#fail({ kind: 'disconnected', detail: UNREADABLE_ANSWER });
  }

  /** Settles the request an answer belongs to; `outcome` is null when the answer could not be read. */
  #onResponse(id: number, outcome: Outcome | null): void {
    const pending = this.#pending.get(id);
    if (pending !== undefined) {
      this.#pending.delete(id);
      clearTimeout(pending.timer);
      if (outcome === null) pending.reject(new RequestError('unreadable', UNREADABLE_ANSWER));
      else if (outcome.status === 'ok') pending.resolve(outcome.value);
      else pending.reject(new RequestError('app', outcome.value.message, outcome.value));
    } else {
      const late = this.#late.get(id);
      if (late === undefined) return; // Unknown id, including UNKNOWN_REQUEST_ID: nothing waits for it.
      this.#late.delete(id);
      for (const listener of this.#lateListeners) listener(late, outcome);
    }
    // The bridge never reconnects to the app within one port (a new app connection must
    // start with hello), so close this port and connect again later with a fresh hello.
    const problem = outcome?.status === 'err' ? connectionProblem(outcome.value) : null;
    if (problem !== null) this.#fail(problem);
  }

  #rememberLate(id: number, call: Call): void {
    this.#late.set(id, call);
    if (this.#late.size > LATE_MEMORY) {
      const oldest = this.#late.keys().next().value;
      if (oldest !== undefined) this.#late.delete(oldest);
    }
  }

  #fail(problem: Problem): void {
    const startNow = problem.kind === 'app_closed' && this.#startWanted && !this.#stopped;
    this.#dropPort();
    if (startNow) {
      // Asked for while an attempt that may not start the app was open, and the app is closed: start it now, for the requests still waiting.
      void this.#connect(true);
      return;
    }
    this.#rejectAll(
      new RequestError('disconnected', problem.detail ?? 'The connection to Wirecatch was lost.'),
    );
    if (this.#stopped) return;
    this.#clearRetry();
    if (problem.kind === 'app_closed') {
      // Closed by the person: nothing retries. The next request, or the popup's Open button, starts it.
      this.#attempt = 0;
      this.#setStatus({ state: 'unavailable', problem, retryAt: null });
      return;
    }
    const delay = backoffDelay(this.#backoff, this.#attempt);
    this.#attempt += 1;
    this.#retryTimer = setTimeout(() => {
      this.#retryTimer = null;
      // Retrying on its own, so it only reaches an app that is running: if the app was closed, the answer says so and retrying stops.
      void this.#connect(false);
    }, delay);
    this.#setStatus({ state: 'unavailable', problem, retryAt: this.#now() + delay });
  }

  #rejectAll(error: RequestError): void {
    for (const [id, pending] of this.#pending) {
      clearTimeout(pending.timer);
      this.#pending.delete(id);
      pending.reject(error);
    }
    for (const waiter of this.#readyWaiters) waiter.reject(error);
    this.#readyWaiters.clear();
  }

  #dropPort(): void {
    this.#generation += 1;
    // Request ids belong to one connection; answers to them can no longer arrive.
    this.#late.clear();
    const port = this.#port;
    this.#port = null;
    if (port === null) return;
    try {
      port.disconnect();
    } catch {
      // Already closed by the browser.
    }
  }

  #clearRetry(): void {
    if (this.#retryTimer !== null) {
      clearTimeout(this.#retryTimer);
      this.#retryTimer = null;
    }
  }

  #setStatus(status: ConnectionStatus): void {
    this.#status = status;
    for (const listener of this.#statusListeners) listener(status);
  }
}
