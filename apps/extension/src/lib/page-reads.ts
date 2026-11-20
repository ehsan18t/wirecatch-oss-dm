import {
  MAX_PAGE_READ_LEN,
  MAX_PAGE_READ_PART,
  PAGE_READ_TIMEOUT_MS,
  type Call,
  type Event,
  type PageRead,
  type PageReadFailure,
  type PageReadId,
} from '@wirecatch/protocol';

/**
 * Page reads for the app's media reads (D282): some sites refuse yt-dlp for not being a browser, so the app hands its requests to the page's site over, and the browser makes them with its own connection and cookies. The answer goes back in parts, in order.
 */
export interface PageReadDeps {
  fetch: (url: string, init: RequestInit) => Promise<Response>;
  /** Sends a call to the app and waits for its answer. */
  send: (call: Call, timeoutMs: number) => Promise<unknown>;
}

/** What one page read sends the app on its way: answered, failed, or nothing for a read the app stopped. */
type Ending =
  | { kind: 'answered'; response: Response; body: Uint8Array }
  | { kind: 'failed'; reason: PageReadFailure }
  | { kind: 'stopped' };

export class PageReads {
  readonly #deps: PageReadDeps;
  readonly #running = new Map<PageReadId, AbortController>();

  constructor(deps: PageReadDeps) {
    this.#deps = deps;
  }

  /** Acts on the app's page read events; leaves every other event. */
  follow(event: Event): void {
    if (event.type === 'page_read_requested') void this.#read(event.data);
    if (event.type === 'page_read_cancelled') this.#running.get(event.data.id)?.abort();
  }

  async #read(read: PageRead): Promise<void> {
    const stop = new AbortController();
    this.#running.set(read.id, stop);
    try {
      const ending = await this.#make(read, stop.signal);
      if (ending.kind === 'stopped') return;
      if (ending.kind === 'failed') {
        await this.#send({
          method: 'page_read_failed',
          params: { id: read.id, reason: ending.reason },
        });
        return;
      }
      for (let at = 0; at < ending.body.length; at += MAX_PAGE_READ_PART) {
        if (stop.signal.aborted) return;
        const bytes = toBase64(ending.body.subarray(at, at + MAX_PAGE_READ_PART));
        await this.#send({ method: 'page_read_part', params: { id: read.id, bytes } });
      }
      const headers = [...ending.response.headers].map(([name, value]) => ({ name, value }));
      await this.#send({
        method: 'page_read_answered',
        params: { id: read.id, status: ending.response.status, headers },
      });
    } catch {
      // The connection to the app broke: the app fails the read itself.
    } finally {
      this.#running.delete(read.id);
    }
  }

  async #make(read: PageRead, signal: AbortSignal): Promise<Ending> {
    const headers = new Headers();
    for (const header of read.headers) {
      try {
        headers.append(header.name, header.value);
      } catch {
        // A header fetch() refuses goes without it, as yt-dlp's other headers do.
      }
    }
    let response: Response;
    try {
      response = await this.#deps.fetch(read.url, {
        method: read.method,
        headers,
        body: read.body === undefined ? undefined : fromBase64(read.body),
        credentials: 'include',
        // Not followed: the app checks every address against the local network before the browser reaches it, and a redirect's would go unchecked.
        redirect: 'error',
        cache: 'no-store',
        signal,
      });
    } catch {
      // The app stopped the read, or the site could not be reached: either is the whole answer.
      return signal.aborted ? { kind: 'stopped' } : { kind: 'failed', reason: 'network' };
    }
    if (Number(response.headers.get('content-length')) > MAX_PAGE_READ_LEN) {
      void response.body?.cancel();
      return { kind: 'failed', reason: 'too_large' };
    }
    try {
      const body = await readCapped(response, MAX_PAGE_READ_LEN);
      return body === null
        ? { kind: 'failed', reason: 'too_large' }
        : { kind: 'answered', response, body };
    } catch {
      // The app stopped the read, or the connection broke mid-body: either is the whole answer.
      return signal.aborted ? { kind: 'stopped' } : { kind: 'failed', reason: 'network' };
    }
  }

  #send(call: Call): Promise<unknown> {
    return this.#deps.send(call, PAGE_READ_TIMEOUT_MS);
  }
}

/** The whole body, or null past `limit` bytes, read no further. */
async function readCapped(response: Response, limit: number): Promise<Uint8Array | null> {
  const reader = response.body?.getReader();
  if (reader === undefined) return new Uint8Array();
  const chunks: Uint8Array[] = [];
  let length = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.length;
    if (length > limit) {
      void reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const body = new Uint8Array(length);
  let at = 0;
  for (const chunk of chunks) {
    body.set(chunk, at);
    at += chunk.length;
  }
  return body;
}

/** `btoa` takes a string of bytes, built in slices small enough for `String.fromCharCode`'s arguments. */
function toBase64(bytes: Uint8Array): string {
  const SLICE = 0x8000;
  let binary = '';
  for (let at = 0; at < bytes.length; at += SLICE) {
    binary += String.fromCharCode(...bytes.subarray(at, at + SLICE));
  }
  return btoa(binary);
}

function fromBase64(text: string): Uint8Array<ArrayBuffer> {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let at = 0; at < binary.length; at += 1) bytes[at] = binary.charCodeAt(at);
  return bytes;
}
