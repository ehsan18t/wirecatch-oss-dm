import { MAX_PAGE_READ_PART, pageReadId, type Call, type PageRead } from '@wirecatch/protocol';
import { describe, expect, it, vi } from 'vitest';
import { browserProfile, PROFILE_KEY } from '../src/lib/browser-profile';
import { PageReads, type PageReadDeps } from '../src/lib/page-reads';
import { ADDRESS_WAIT_MS, ReachedAddresses } from '../src/lib/page-read-addresses';

const ID = pageReadId(7);
const ADDRESS = '93.184.215.14';
const ORIGIN = 'chrome-extension://abc';

function read(method = 'GET'): PageRead {
  return {
    id: ID,
    method,
    url: 'https://www.example.com/v',
    headers: [{ name: 'Accept', value: '*/*' }],
  };
}

/** Page reads whose fetch answers with `respond`, keeping what they send the app; `sent` resolves at the answer or the failure. */
function reads(respond: () => Promise<Response>, deps: Partial<PageReadDeps> = {}) {
  const sent: Call[] = [];
  let ended = (): void => undefined;
  const done = new Promise<void>((resolve) => (ended = resolve));
  const asked: RequestInit[] = [];
  const pageReads = new PageReads({
    fetch: (_url, init) => {
      asked.push(init);
      return respond();
    },
    send: (call) => {
      sent.push(call);
      if (call.method !== 'page_read_part') ended();
      return Promise.resolve({ type: 'done' });
    },
    addressOf: () => Promise.resolve(ADDRESS),
    now: () => 0,
    ...deps,
  });
  return { pageReads, sent, asked, done };
}

describe('page reads (D282)', () => {
  it('answers a read in parts, in order, then with its status, headers and address', async () => {
    const body = new Uint8Array(MAX_PAGE_READ_PART + 3).fill(65);
    const { pageReads, sent, asked, done } = reads(() =>
      Promise.resolve(
        new Response(body, { status: 410, headers: { 'Content-Type': 'text/html' } }),
      ),
    );
    pageReads.follow({ type: 'page_read_requested', data: read() });
    await done;
    expect(asked[0]).toMatchObject({ method: 'GET', credentials: 'include', redirect: 'error' });
    expect(sent.map((call) => call.method)).toEqual([
      'page_read_part',
      'page_read_part',
      'page_read_answered',
    ]);
    const parts = sent.flatMap((call) =>
      call.method === 'page_read_part' ? [atob(call.params.bytes).length] : [],
    );
    expect(parts).toEqual([MAX_PAGE_READ_PART, 3]);
    expect(sent[2]).toEqual({
      method: 'page_read_answered',
      params: {
        id: ID,
        status: 410,
        headers: [{ name: 'content-type', value: 'text/html' }],
        address: ADDRESS,
      },
    });
  });

  it('answers a HEAD whose stated length is past the limit, since its body is empty', async () => {
    const { pageReads, sent, done } = reads(() =>
      Promise.resolve(
        new Response(null, { status: 200, headers: { 'Content-Length': String(2 ** 40) } }),
      ),
    );
    pageReads.follow({ type: 'page_read_requested', data: read('HEAD') });
    await done;
    expect(sent.map((call) => call.method)).toEqual(['page_read_answered']);
  });

  it('leaves out an address the browser did not report', async () => {
    const { pageReads, sent, done } = reads(() => Promise.resolve(new Response('page')), {
      addressOf: () => Promise.resolve(null),
    });
    pageReads.follow({ type: 'page_read_requested', data: read() });
    await done;
    const answered = sent.find((call) => call.method === 'page_read_answered');
    expect(answered?.params).not.toHaveProperty('address');
  });

  it('stops every read when the connection to the app goes, and sends nothing for them', async () => {
    let fetching = (): void => undefined;
    const started = new Promise<void>((resolve) => (fetching = resolve));
    const sent: Call[] = [];
    const pageReads = new PageReads({
      // Like `fetch()`, it rejects once its signal aborts.
      fetch: (_url, init) => {
        fetching();
        return new Promise<Response>((_resolve, reject) => {
          init.signal?.addEventListener('abort', () => reject(new DOMException('', 'AbortError')));
        });
      },
      send: (call) => {
        sent.push(call);
        return Promise.resolve({ type: 'done' });
      },
      addressOf: () => Promise.resolve(ADDRESS),
      now: () => 0,
    });
    pageReads.follow({ type: 'page_read_requested', data: read() });
    await started;
    pageReads.disconnected();
    // A task runs only once every reaction to the abort has.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(sent).toEqual([]);
  });
});

describe('reached addresses (D282)', () => {
  const ours = { url: 'https://www.example.com/v', tabId: -1, initiator: ORIGIN };

  it("takes only this extension's own requests", async () => {
    vi.useFakeTimers();
    try {
      const reached = new ReachedAddresses(ORIGIN, () => 10);
      reached.seen({ ...ours, tabId: 3, ip: '10.0.0.1' });
      reached.seen({ ...ours, initiator: 'https://www.example.com', ip: '10.0.0.2' });
      reached.seen({ ...ours, ip: ADDRESS });
      await expect(reached.of(ours.url, 0)).resolves.toBe(ADDRESS);
    } finally {
      vi.useRealTimers();
    }
  });

  it('waits for an address that comes after the answer, and gives up after its wait', async () => {
    vi.useFakeTimers();
    try {
      const reached = new ReachedAddresses(ORIGIN, () => 10);
      const later = reached.of(ours.url, 0);
      reached.seen({ ...ours, ip: ADDRESS });
      await expect(later).resolves.toBe(ADDRESS);
      const never = reached.of('https://www.example.com/other', 0);
      await vi.advanceTimersByTimeAsync(ADDRESS_WAIT_MS);
      await expect(never).resolves.toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('says nothing when two reads of one address went to different places', async () => {
    const reached = new ReachedAddresses(ORIGIN, () => 10);
    reached.seen({ ...ours, ip: ADDRESS });
    reached.seen({ ...ours, ip: '192.168.0.1' });
    await expect(reached.of(ours.url, 0)).resolves.toBeNull();
  });
});

describe('browser profile (D282)', () => {
  it('is made once and kept', async () => {
    const stored: Record<string, unknown> = {};
    const area = {
      get: (key: string) => Promise.resolve({ [key]: stored[key] }),
      set: (items: Record<string, unknown>) => {
        Object.assign(stored, items);
        return Promise.resolve();
      },
    };
    const first = await browserProfile(area, () => 'profile-1');
    const second = await browserProfile(area, () => 'profile-2');
    expect([first, second, stored[PROFILE_KEY]]).toEqual(['profile-1', 'profile-1', 'profile-1']);
  });
});
