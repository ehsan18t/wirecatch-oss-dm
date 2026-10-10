import type { Call, Event, Hello, Message, ProtocolError, Reply } from '@wirecatch/protocol';
import { PROTOCOL_VERSION } from '../protocol';
import { classifyDisconnect, type ConnectionStatus, type Problem } from './status';

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
  /** Builds the `hello` call sent first on every connection. */
  hello: () => Promise<Hello>;
  helloTimeoutMs?: number;
  /** Delays between reconnection attempts; the last one repeats. */
  backoffMs?: readonly number[];
  now?: () => number;
}

export type RequestErrorKind =
  /** No answer in time. */
  | 'timeout'
  /** No connection to the app, or it broke before the answer came. */
  | 'disconnected'
  /** The app answered with an error. */
  | 'app';

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
  resolve(reply: Reply): void;
  reject(error: RequestError): void;
  timer: ReturnType<typeof setTimeout>;
}

const DEFAULT_BACKOFF = [1_000, 2_000, 4_000, 8_000, 15_000, 30_000] as const;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

/** Checks the outer shape of a frame from the bridge; anything else is ignored. */
function asMessage(raw: unknown): Message | null {
  if (!isRecord(raw)) return null;
  if (raw.kind === 'response' && typeof raw.id === 'number' && isRecord(raw.outcome)) {
    const status = raw.outcome.status;
    if ((status === 'ok' || status === 'err') && isRecord(raw.outcome.value)) return raw as Message;
    return null;
  }
  if (raw.kind === 'event' && isRecord(raw.event) && typeof raw.event.type === 'string') {
    return raw as Message;
  }
  return null;
}

function problemFromHelloError(error: RequestError): Problem {
  const code = error.protocolError?.code;
  if (code === 'app_unavailable') return { kind: 'app_unavailable', detail: error.message };
  if (code === 'unsupported_version') return { kind: 'version_mismatch', detail: error.message };
  return { kind: 'disconnected', detail: error.message };
}

/**
 * Talks to the Wirecatch app through the native messaging bridge.
 *
 * Sends `hello` first on every connection, matches answers to requests by id,
 * times requests out, passes app events on, and reconnects with backoff when the
 * connection breaks.
 */
export class NativeClient {
  readonly #options: ClientOptions;
  readonly #backoff: readonly number[];
  readonly #now: () => number;
  #status: ConnectionStatus = { state: 'connecting' };
  #port: NativePort | null = null;
  /** Bumped whenever a port is dropped, so late events from it are ignored. */
  #generation = 0;
  #nextId = 1;
  #attempt = 0;
  #started = false;
  #stopped = false;
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

  /** Connects for the first time. Calling it again does nothing. */
  start(): void {
    if (this.#started) return;
    this.#started = true;
    void this.#connect();
  }

  /** Closes the connection for good and fails everything still waiting. */
  stop(): void {
    this.#stopped = true;
    this.#clearRetry();
    this.#dropPort();
    this.#rejectAll(new RequestError('disconnected', 'The connection to Wirecatch was closed.'));
  }

  /** Skips the remaining backoff and tries to connect right away (the popup's "Try again"). */
  reconnectNow(): void {
    if (this.#stopped || this.#status.state === 'connected' || this.#isAttempting()) return;
    this.#clearRetry();
    void this.#connect();
  }

  onStatus(listener: (status: ConnectionStatus) => void): () => void {
    this.#statusListeners.add(listener);
    return () => this.#statusListeners.delete(listener);
  }

  onEvent(listener: (event: Event) => void): () => void {
    this.#eventListeners.add(listener);
    return () => this.#eventListeners.delete(listener);
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
      return Promise.reject(new RequestError('disconnected', 'The connection to Wirecatch was closed.'));
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
      if (!this.#isAttempting()) this.reconnectNow();
    });
  }

  async #connect(): Promise<void> {
    this.#dropPort();
    const generation = this.#generation;
    if (this.#status.state === 'unavailable') {
      this.#setStatus({ ...this.#status, retryAt: null });
    }

    let port: NativePort;
    try {
      port = this.#options.transport.connect();
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      this.#fail({ kind: classifyDisconnect(detail), detail });
      return;
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

    let reply: Reply;
    try {
      const hello = await this.#options.hello();
      if (generation !== this.#generation) return;
      reply = await this.#send({ method: 'hello', params: hello }, this.#options.helloTimeoutMs ?? 10_000);
    } catch (error) {
      if (generation !== this.#generation) return;
      const failure = error instanceof RequestError ? error : new RequestError('disconnected', String(error));
      this.#fail(problemFromHelloError(failure));
      return;
    }
    if (generation !== this.#generation) return;
    if (reply.type !== 'hello') {
      this.#fail({ kind: 'disconnected', detail: 'Wirecatch gave an unexpected answer to hello.' });
      return;
    }
    if (reply.data.protocol_version !== PROTOCOL_VERSION) {
      this.#fail({
        kind: 'version_mismatch',
        detail: `The app speaks protocol version ${reply.data.protocol_version}; this extension speaks version ${PROTOCOL_VERSION}.`,
      });
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
    return new Promise<Reply>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new RequestError('timeout', 'Wirecatch did not answer in time.'));
      }, timeoutMs);
      this.#pending.set(id, { resolve, reject, timer });
      try {
        port.postMessage({ kind: 'request', id, call });
      } catch (error) {
        clearTimeout(timer);
        this.#pending.delete(id);
        const detail = error instanceof Error ? error.message : String(error);
        reject(new RequestError('disconnected', `Could not send to Wirecatch: ${detail}`));
      }
    });
  }

  #onMessage(raw: unknown): void {
    const message = asMessage(raw);
    if (message === null) return;
    if (message.kind === 'event') {
      for (const listener of this.#eventListeners) listener(message.event);
      return;
    }
    if (message.kind !== 'response') return;
    const pending = this.#pending.get(message.id);
    if (pending === undefined) return;
    this.#pending.delete(message.id);
    clearTimeout(pending.timer);
    const { outcome } = message;
    if (outcome.status === 'ok') {
      pending.resolve(outcome.value);
      return;
    }
    pending.reject(new RequestError('app', outcome.value.message, outcome.value));
    // The bridge never reconnects to the app within one port (a new app connection must
    // start with hello), so close this port and connect again later with a fresh hello.
    if (outcome.value.code === 'app_unavailable') {
      this.#fail({ kind: 'app_unavailable', detail: outcome.value.message });
    }
  }

  #fail(problem: Problem): void {
    this.#dropPort();
    this.#rejectAll(
      new RequestError('disconnected', problem.detail ?? 'The connection to Wirecatch was lost.'),
    );
    if (this.#stopped) return;
    const delay = this.#backoff[Math.min(this.#attempt, this.#backoff.length - 1)] ?? 30_000;
    this.#attempt += 1;
    this.#clearRetry();
    this.#retryTimer = setTimeout(() => {
      this.#retryTimer = null;
      void this.#connect();
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
