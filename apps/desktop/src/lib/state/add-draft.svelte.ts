// The Add dialog's form while it is open: the links as typed, the file name, the folder and the queue, and what went wrong. It lives in the state rather than in the dialog, so links that reach the window while it is open (from a second launch) join what the person typed instead of replacing it (DUI-8).

import type { AddRequest, DownloadId, QueueId } from '@wirecatch/protocol';
import { extractUrls } from '../format';
import type { Added } from './download-actions.svelte';
import { errorMessage } from './runner';

/** Said when the box holds no link to send. */
export const NO_LINK = 'Paste at least one link that starts with https://, http:// or ftp://.';

const PICKER_FAILED = 'Could not open the folder picker';

/** What the links box holds: the links in it, each once, and how many lines have none. */
export interface LinksRead {
  urls: string[];
  /** Lines with text but no link, which are left out. */
  ignored: number;
}

export function readLinks(text: string): LinksRead {
  const urls = extractUrls(text);
  const ignored = text
    .split('\n')
    .filter((line) => line.trim() !== '' && extractUrls(line).length === 0).length;
  return { urls, ignored };
}

/** The hint under the links box: how to use it, or what it leaves out. */
export function linksHint({ ignored }: LinksRead): string {
  if (ignored === 0) return 'One link per line, or paste several at once.';
  if (ignored === 1) return 'One line has no link in it and is left out.';
  return `${String(ignored)} lines have no link in them and are left out.`;
}

/** Something the app could not do for the form, shown in it: the dialog covers the window's notes. */
export interface Problem {
  title: string;
  message: string;
}

/** What the form needs from the window. */
export interface AddDraftHost {
  add: (requests: AddRequest[]) => Promise<Added>;
  /** Asks for a folder, starting at `current`; null when none was picked. Rejects when the picker fails. */
  pickFolder: (current?: string) => Promise<string | null>;
  /** Some links became downloads: the list shows them. */
  added: (ids: DownloadId[]) => void;
  /** Every link became a download: the form's work is done. Closes this form's dialog only, so a send that ends after it was closed leaves the next one open. */
  close: () => void;
}

export class AddDraft {
  /** The links box, as typed. */
  text = $state('');
  /** The name for a single link; empty for the name the server gives. */
  fileName = $state('');
  /** The folder to save in; null for the one Settings picks by file type. */
  folder = $state<string | null>(null);
  /** The queue; null lets the core use its default (none listed yet). */
  queue = $state<QueueId | null>(null);
  /** Why the links cannot be sent as they are. */
  invalid = $state<string | null>(null);
  problem = $state<Problem | null>(null);
  busy = $state(false);
  /** Opened with links (pasted, dropped, from the command line): they are ready to send as they are. */
  readonly fromPaste: boolean;

  readonly links = $derived(readLinks(this.text));
  /** The one link, when there is one: only then does the name apply. */
  readonly single = $derived(this.links.urls.length === 1 ? this.links.urls[0] : undefined);

  readonly #host: AddDraftHost;

  constructor(host: AddDraftHost, start: { urls: string[]; queue: QueueId | null }) {
    this.#host = host;
    this.text = start.urls.join('\n');
    this.queue = start.queue;
    this.fromPaste = start.urls.length > 0;
  }

  /** The person typed in the links box. */
  edit(text: string): void {
    this.text = text;
    this.invalid = null;
  }

  /** Links from outside the form join it, after what is there, without repeating one. */
  append(urls: string[]): void {
    const fresh = readLinks(urls.join('\n')).urls.filter((url) => !this.links.urls.includes(url));
    if (fresh.length === 0) return;
    const kept = this.text.trimEnd();
    this.edit(kept === '' ? fresh.join('\n') : `${kept}\n${fresh.join('\n')}`);
  }

  async chooseFolder(): Promise<void> {
    try {
      const picked = await this.#host.pickFolder(this.folder ?? undefined);
      // The picker works again: its earlier failure no longer applies.
      if (this.problem?.title === PICKER_FAILED) this.problem = null;
      if (picked !== null) this.folder = picked;
    } catch (err) {
      this.problem = { title: PICKER_FAILED, message: errorMessage(err) };
    }
  }

  useDefaultFolder(): void {
    this.folder = null;
  }

  /** Sends every link, started now or paused. True when all became downloads and the form closed. Otherwise the box keeps what is left to send: the links that failed, with why, then any that joined it while it sent (from a second launch, or typed). */
  async submit(start: boolean): Promise<boolean> {
    if (this.busy) return false;
    const { urls } = this.links;
    if (urls.length === 0) {
      this.invalid = NO_LINK;
      return false;
    }
    const name = this.single === undefined ? '' : this.fileName.trim();
    const requests = urls.map((url) => ({
      url,
      queue: this.queue,
      folder: this.folder,
      file_name: name === '' ? null : name,
      start,
    }));
    this.busy = true;
    this.problem = null;
    const { ids, failed } = await this.#host.add(requests).finally(() => {
      this.busy = false;
    });
    if (ids.length > 0) this.#host.added(ids);
    const later = this.links.urls.filter((url) => !urls.includes(url));
    const left = [...failed.map((f) => f.url), ...later];
    if (left.length === 0) {
      this.#host.close();
      return true;
    }
    this.text = left.join('\n');
    const [first] = failed;
    if (first === undefined) return false;
    this.problem =
      urls.length === 1
        ? { title: 'Could not add the link', message: first.error }
        : {
            title: `Could not add ${String(failed.length)} of ${String(urls.length)} links`,
            message: `${first.error} ${failed.length === 1 ? 'The link stays' : 'The links stay'} here to try again.`,
          };
    return false;
  }
}
