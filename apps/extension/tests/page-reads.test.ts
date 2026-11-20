import { MAX_PAGE_READ_PART, pageReadId, type Call } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import { PageReads } from '../src/lib/page-reads';

const ID = pageReadId(7);

describe('page reads (D282)', () => {
  it('answers a read in parts, in order, then with its status and final address', async () => {
    const body = new Uint8Array(MAX_PAGE_READ_PART + 3).fill(65);
    const sent: Call[] = [];
    let answered = (): void => undefined;
    const done = new Promise<void>((resolve) => (answered = resolve));
    const asked: RequestInit[] = [];
    const reads = new PageReads({
      fetch: (_url, init) => {
        asked.push(init);
        const response = new Response(body, {
          status: 410,
          headers: { 'Content-Type': 'text/html' },
        });
        Object.defineProperty(response, 'url', { value: 'https://www.example.com/final' });
        return Promise.resolve(response);
      },
      send: (call) => {
        sent.push(call);
        if (call.method === 'page_read_answered') answered();
        return Promise.resolve({ type: 'done' });
      },
    });
    reads.follow({
      type: 'page_read_requested',
      data: {
        id: ID,
        method: 'GET',
        url: 'https://www.example.com/v',
        headers: [{ name: 'Accept', value: '*/*' }],
      },
    });
    await done;
    expect(asked[0]).toMatchObject({ method: 'GET', credentials: 'include', redirect: 'follow' });
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
        url: 'https://www.example.com/final',
        headers: [{ name: 'content-type', value: 'text/html' }],
      },
    });
  });
});
