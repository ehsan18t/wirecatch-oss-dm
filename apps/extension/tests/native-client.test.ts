import { UNKNOWN_REQUEST_ID, type Event, type Hello } from '@wirecatch/protocol';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MAX_MESSAGE_BYTES, NativeClient, RequestError } from '../src/lib/native/client';
import type { ConnectionStatus } from '../src/lib/native/status';
import { patchOf } from '../src/lib/protocol';
import { FakeApp, flush } from './fakes';

const HELLO: Omit<Hello, 'start_app'> = {
  client: 'extension',
  client_version: '0.1.0',
  protocol_version: 1,
  browser: { name: 'chrome', version: '141' },
};

function makeClient(app: FakeApp, backoffMs: number[] = [1_000, 2_000, 4_000]): NativeClient {
  return new NativeClient({
    transport: app,
    hello: () => Promise.resolve(HELLO),
    helloTimeoutMs: 5_000,
    backoffMs,
  });
}

describe('NativeClient', () => {
  let app: FakeApp;
  let client: NativeClient;

  beforeEach(() => {
    vi.useFakeTimers();
    app = new FakeApp();
  });

  afterEach(() => {
    client.stop();
    vi.useRealTimers();
  });

  it('sends hello first, with client kind, protocol version and browser', async () => {
    client = makeClient(app);
    client.start();
    await flush();
    // Starting up only reaches an app that is already running.
    expect(app.requests[0]).toEqual({
      id: 1,
      call: { method: 'hello', params: { ...HELLO, start_app: false } },
    });
    expect(client.status).toEqual({ state: 'connected', appVersion: '0.1.0' });
  });

  // Quitting Wirecatch did not stick: the extension reconnected within a second, and every new
  // bridge started the app again, so it came back hidden while any browser window was open.
  it('keeps a Wirecatch the person closed closed', async () => {
    client = makeClient(app);
    client.start();
    await flush();
    expect(client.status.state).toBe('connected');

    app.running = false;
    app.openPort?.browserDisconnect('Native host has exited.');
    await vi.advanceTimersByTimeAsync(60_000);

    expect(client.status).toEqual({
      state: 'unavailable',
      problem: { kind: 'app_closed', detail: null },
      retryAt: null,
    });
    expect(app.starts).toBe(0);
    const connections = app.ports.length;
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(app.ports).toHaveLength(connections);
  });

  it('does not start a closed Wirecatch when the extension starts up', async () => {
    app.running = false;
    client = makeClient(app);
    client.start();
    await flush();
    expect(client.status.state).toBe('unavailable');
    expect(app.starts).toBe(0);
  });

  it('starts a closed Wirecatch for a request, which only the person causes', async () => {
    app.running = false;
    client = makeClient(app);
    client.start();
    await flush();

    const reply = await client.request({ method: 'get_extension_settings' }, 5_000);

    expect(reply.type).toBe('extension_settings');
    expect(app.starts).toBe(1);
    expect(client.status.state).toBe('connected');
  });

  // The background usually wakes for the very click or download that needs the app, so the request
  // arrives while the start-up attempt, which never starts the app, is still open. It failed with
  // "The connection to Wirecatch was lost." and the app was never started.
  it('starts a closed Wirecatch for a request made while the start-up attempt is open', async () => {
    app.running = false;
    client = makeClient(app);
    client.start();

    const reply = await client.request({ method: 'get_extension_settings' }, 5_000);

    expect(reply.type).toBe('extension_settings');
    expect(app.starts).toBe(1);
  });

  it('starts a closed Wirecatch for the Open button pressed while the start-up attempt is open', async () => {
    app.running = false;
    client = makeClient(app);
    client.start();
    client.reconnectNow();
    await flush();

    expect(app.starts).toBe(1);
    expect(client.status.state).toBe('connected');
  });

  it('matches answers to requests by id, even when they arrive out of order', async () => {
    client = makeClient(app);
    client.start();
    await flush();
    app.answers = false;
    const settings = client.request({ method: 'get_extension_settings' }, 1_000);
    const queues = client.request({ method: 'list_queues' }, 1_000);
    await flush();
    const [first, second] = app.requests.slice(-2);
    const port = app.openPort;
    if (first === undefined || second === undefined || port === undefined)
      throw new Error('requests missing');
    port.deliver({
      kind: 'response',
      id: second.id,
      outcome: { status: 'ok', value: { type: 'queues', data: [] } },
    });
    port.deliver({
      kind: 'response',
      id: first.id,
      outcome: { status: 'ok', value: { type: 'extension_settings', data: app.settings } },
    });
    expect((await settings).type).toBe('extension_settings');
    expect((await queues).type).toBe('queues');
  });

  it('ignores answers to unknown ids and malformed frames', async () => {
    client = makeClient(app);
    client.start();
    await flush();
    const port = app.openPort;
    if (port === undefined) throw new Error('no port');
    port.deliver({ kind: 'response', id: 999, outcome: { status: 'ok', value: { type: 'done' } } });
    port.deliver({ nonsense: true } as never);
    await flush();
    expect(client.status.state).toBe('connected');
  });

  it('times a request out without dropping the connection', async () => {
    client = makeClient(app);
    client.start();
    await flush();
    app.answers = false;
    const result = client.request({ method: 'pause_all' }, 2_000).catch((e: unknown) => e);
    await vi.advanceTimersByTimeAsync(2_000);
    const error = await result;
    expect(error).toBeInstanceOf(RequestError);
    expect((error as RequestError).kind).toBe('timeout');
    expect(client.status.state).toBe('connected');
  });

  it('fails an oversized request on its own, without sending it or dropping the connection', async () => {
    client = makeClient(app);
    client.start();
    await flush();
    const port = app.openPort;
    const huge = 'x'.repeat(MAX_MESSAGE_BYTES);
    const error = await client
      .request(
        { method: 'update_extension_settings', params: patchOf({ add_ignored_sites: [huge] }) },
        1_000,
      )
      .catch((e: unknown) => e);
    expect(error).toMatchObject({ kind: 'too_large' });
    expect((error as RequestError).message).toContain('too large');
    expect(app.calls('update_extension_settings')).toHaveLength(0);
    expect(client.status.state).toBe('connected');
    expect(app.openPort).toBe(port);
    expect(await client.request({ method: 'pause_all' }, 1_000)).toEqual({ type: 'done' });
  });

  it('counts the limit in UTF-8 bytes, not characters', async () => {
    client = makeClient(app);
    client.start();
    await flush();
    // About 0.4 M characters, but each is 3 bytes in UTF-8: over 1 MiB.
    const wide = '€'.repeat(400_000);
    const error = await client
      .request(
        { method: 'update_extension_settings', params: patchOf({ add_ignored_sites: [wide] }) },
        1_000,
      )
      .catch((e: unknown) => e);
    expect(error).toMatchObject({ kind: 'too_large' });
  });

  it('passes an answer that arrives after the timeout to late-answer listeners', async () => {
    client = makeClient(app);
    const late: Array<[string, string]> = [];
    client.onLateAnswer((call, outcome) => late.push([call.method, outcome.status]));
    client.start();
    await flush();
    app.answers = false;
    const result = client.request({ method: 'pause_all' }, 1_000).catch((e: unknown) => e);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(await result).toMatchObject({ kind: 'timeout' });
    const sent = app.requests.at(-1);
    if (sent === undefined) throw new Error('nothing sent');
    app.openPort?.deliver({
      kind: 'response',
      id: sent.id,
      outcome: { status: 'ok', value: { type: 'done' } },
    });
    await flush();
    expect(late).toEqual([['pause_all', 'ok']]);
    // Only once.
    app.openPort?.deliver({
      kind: 'response',
      id: sent.id,
      outcome: { status: 'ok', value: { type: 'done' } },
    });
    await flush();
    expect(late).toHaveLength(1);
  });

  it('never uses request id 0 and ignores answers carrying it', async () => {
    client = makeClient(app);
    client.start();
    await flush();
    app.answers = false;
    const pending = client.request({ method: 'pause_all' }, 1_000).catch((e: unknown) => e);
    await flush();
    expect(app.requests.map((r) => r.id)).not.toContain(UNKNOWN_REQUEST_ID);
    app.openPort?.deliver({
      kind: 'response',
      id: UNKNOWN_REQUEST_ID,
      outcome: { status: 'err', value: { code: 'invalid_request', message: 'Bad frame.' } },
    });
    await flush();
    expect(client.status.state).toBe('connected');
    await vi.advanceTimersByTimeAsync(1_000);
    expect(await pending).toMatchObject({ kind: 'timeout' });
  });

  it('passes app errors on with their code', async () => {
    client = makeClient(app);
    client.start();
    await flush();
    app.updateError = { code: 'invalid_request', message: 'No such queue.' };
    const error = await client
      .request({ method: 'update_extension_settings', params: {} }, 1_000)
      .catch((e: unknown) => e);
    expect(error).toMatchObject({
      kind: 'app',
      message: 'No such queue.',
      protocolError: { code: 'invalid_request' },
    });
  });

  it('passes app events to listeners', async () => {
    client = makeClient(app);
    const events: Event[] = [];
    client.onEvent((e) => events.push(e));
    client.start();
    await flush();
    app.changeSettings({ capture_enabled: false });
    await flush();
    expect(events).toEqual([{ type: 'extension_settings_changed', data: app.settings }]);
  });

  it('reports a missing native host and retries with growing delays', async () => {
    app.hostMissing = 'Specified native messaging host not found.';
    client = makeClient(app);
    const statuses: ConnectionStatus[] = [];
    client.onStatus((s) => statuses.push(s));
    client.start();
    await flush();
    expect(client.status).toMatchObject({
      state: 'unavailable',
      problem: { kind: 'host_missing', detail: 'Specified native messaging host not found.' },
    });
    expect(app.ports).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(999);
    expect(app.ports).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(app.ports).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(2_000);
    expect(app.ports).toHaveLength(3);
    await vi.advanceTimersByTimeAsync(4_000);
    expect(app.ports).toHaveLength(4);
    // The last delay repeats.
    await vi.advanceTimersByTimeAsync(4_000);
    expect(app.ports).toHaveLength(5);

    app.hostMissing = null;
    await vi.advanceTimersByTimeAsync(4_000);
    expect(client.status.state).toBe('connected');
    expect(statuses.at(-1)).toEqual({ state: 'connected', appVersion: '0.1.0' });
  });

  it('starts the backoff over after a successful connection', async () => {
    client = makeClient(app);
    client.start();
    await flush();
    app.openPort?.browserDisconnect('Native host has exited.');
    expect(client.status).toMatchObject({
      state: 'unavailable',
      problem: { kind: 'disconnected' },
    });
    await vi.advanceTimersByTimeAsync(1_000);
    expect(client.status.state).toBe('connected');
    app.openPort?.browserDisconnect('Native host has exited.');
    const before = app.ports.length;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(app.ports.length).toBe(before + 1);
  });

  it('shows AppUnavailable from the bridge as its own problem', async () => {
    app.helloError = { code: 'app_unavailable', message: 'Could not start Wirecatch.' };
    client = makeClient(app);
    client.start();
    await flush();
    expect(client.status).toMatchObject({
      state: 'unavailable',
      problem: { kind: 'app_unavailable', detail: 'Could not start Wirecatch.' },
    });
  });

  it('after AppUnavailable on a request, closes the port and reconnects later with a fresh hello', async () => {
    client = makeClient(app);
    client.start();
    await flush();
    const first = app.ports[0];
    if (first === undefined) throw new Error('no port');
    app.updateError = { code: 'app_unavailable', message: 'Wirecatch closed.' };
    const error = await client
      .request({ method: 'update_extension_settings', params: {} }, 1_000)
      .catch((e: unknown) => e);
    expect(error).toMatchObject({ kind: 'app', protocolError: { code: 'app_unavailable' } });
    expect(client.status).toMatchObject({
      state: 'unavailable',
      problem: { kind: 'app_unavailable' },
    });
    // The extension itself closes the port: the bridge never reconnects to the app within one port.
    expect(first.closedByExtension).toBe(true);
    expect(app.ports).toHaveLength(1);
    expect(first.methods).toEqual(['hello', 'update_extension_settings']);

    // The new port waits for the backoff delay.
    app.updateError = null;
    await vi.advanceTimersByTimeAsync(999);
    expect(app.ports).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(app.ports).toHaveLength(2);
    const second = app.ports[1];
    expect(second?.methods[0]).toBe('hello');
    await flush();
    expect(client.status.state).toBe('connected');
    // Later calls go on the new port only.
    await client.request({ method: 'pause_all' }, 1_000);
    expect(first.methods).toEqual(['hello', 'update_extension_settings']);
    expect(second?.methods).toEqual(['hello', 'pause_all']);
  });

  it('after AppUnavailable on hello, closes the port and tries a new one with hello again', async () => {
    app.helloError = { code: 'app_unavailable', message: 'Could not start Wirecatch.' };
    client = makeClient(app);
    client.start();
    await flush();
    const first = app.ports[0];
    expect(first?.closedByExtension).toBe(true);
    expect(first?.methods).toEqual(['hello']);
    app.helloError = null;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(app.ports).toHaveLength(2);
    expect(app.ports[1]?.methods).toEqual(['hello']);
    expect(client.status.state).toBe('connected');
  });

  it('refuses a different protocol version', async () => {
    app.protocolVersion = 2;
    client = makeClient(app);
    client.start();
    await flush();
    expect(client.status).toMatchObject({
      state: 'unavailable',
      problem: { kind: 'version_mismatch' },
    });
    app.protocolVersion = 1;
    app.helloError = { code: 'unsupported_version', message: 'Protocol 1 is not supported.' };
    await vi.advanceTimersByTimeAsync(1_000);
    expect(client.status).toMatchObject({
      state: 'unavailable',
      problem: { kind: 'version_mismatch' },
    });
  });

  it('gives up on a hello that never gets an answer and tries again', async () => {
    app.answers = false;
    client = makeClient(app);
    client.start();
    await vi.advanceTimersByTimeAsync(5_000);
    expect(client.status).toMatchObject({
      state: 'unavailable',
      problem: { kind: 'disconnected' },
    });
    app.answers = true;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(client.status.state).toBe('connected');
  });

  it('fails waiting requests when the connection breaks', async () => {
    client = makeClient(app);
    client.start();
    await flush();
    app.answers = false;
    const pending = client.request({ method: 'pause_all' }, 10_000).catch((e: unknown) => e);
    await flush();
    app.openPort?.browserDisconnect('Native host has exited.');
    expect(await pending).toMatchObject({ kind: 'disconnected' });
  });

  it('connects on demand when a request comes in while disconnected', async () => {
    app.hostMissing = 'Specified native messaging host not found.';
    client = makeClient(app, [60_000]);
    client.start();
    await flush();
    expect(client.status.state).toBe('unavailable');
    app.hostMissing = null;
    const reply = await client.request({ method: 'pause_all' }, 1_000);
    expect(reply).toEqual({ type: 'done' });
  });

  it('fails a request at once when the on-demand attempt fails', async () => {
    app.hostMissing = 'Specified native messaging host not found.';
    client = makeClient(app, [60_000]);
    client.start();
    await flush();
    const error = await client.request({ method: 'pause_all' }, 30_000).catch((e: unknown) => e);
    expect(error).toMatchObject({ kind: 'disconnected' });
  });

  it('try again skips the remaining wait', async () => {
    app.hostMissing = 'Specified native messaging host not found.';
    client = makeClient(app, [60_000]);
    client.start();
    await flush();
    app.hostMissing = null;
    client.reconnectNow();
    await flush();
    expect(client.status.state).toBe('connected');
  });

  it('copes with a browser that throws on connect', async () => {
    client = new NativeClient({
      transport: {
        connect: () => {
          throw new Error('Invalid native messaging host name specified.');
        },
        disconnectReason: () => null,
      },
      hello: () => Promise.resolve(HELLO),
    });
    client.start();
    await flush();
    expect(client.status).toMatchObject({
      state: 'unavailable',
      problem: { kind: 'disconnected' },
    });
  });
});
