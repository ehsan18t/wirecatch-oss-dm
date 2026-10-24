/**
 * The app's notes in the tab the person is looking at (D65). The app asks every connected browser;
 * this one shows a note only while it is the program in front and its tab is a web page whose page
 * script can place the notes frame, and tells the app once the notes page has the note on screen,
 * so the app shows a system notification whenever the note did not show. Each tab's frame proves
 * itself with the token its page script made, so a website that frames the notes page itself gets
 * nothing.
 */
import { NOTE_ANSWER_WAIT_MS, type BrowserNote, type NoteId } from '@wirecatch/protocol';
import { fallback, log } from '../diag/log';
import { isFromNotesPage, NOTES_PORT, type FromNotesPage, type ToNotesPage } from './messages';

/**
 * How long a frame has to connect before it is taken away: its page left, its website removed it,
 * or did not let it load. The app has told its notes another way by then.
 */
export const CONNECT_WAIT_MS = 3000;

/** What the note center needs from the browser and the app. */
export interface NoteCenterDeps {
  /** The tab the person is looking at, while the browser is in front and the tab is a web page; null otherwise. */
  focusedTab: () => Promise<number | null>;
  /** Asks the tab's page script to place a fresh notes frame: its token, or null when the page cannot. */
  mount: (tabId: number) => Promise<string | null>;
  /** Asks the tab's page script to take the notes frame away. */
  unmount: (tabId: number) => Promise<void>;
  /** Tells the app the note is on screen. */
  shown: (id: NoteId) => Promise<void>;
  /** Tells the app the person pressed one of the note's buttons. */
  pressed: (id: NoteId, button: number) => Promise<void>;
  /** Milliseconds since some fixed point. */
  now: () => number;
}

/** The part of a `runtime.Port` the note center uses. */
export interface NotesPort {
  name: string;
  postMessage: (message: ToNotesPage) => void;
  disconnect: () => void;
  onMessage: { addListener: (listener: (message: unknown) => void) => void };
  onDisconnect: { addListener: (listener: () => void) => void };
}

/** One tab's notes frame. */
interface Frame {
  token: string;
  /** The notes page's port, once it has proved itself. */
  port: NotesPort | null;
  /** Notes waiting for the page to connect, with when they came. */
  waiting: { note: BrowserNote; at: number }[];
  /** The notes sent to this tab, whose buttons it may press. */
  sent: Set<NoteId>;
  /** The last note sent, so an `empty` that crossed a newer note does not close the frame. */
  last: NoteId | null;
}

export class NoteCenter {
  readonly #deps: NoteCenterDeps;
  readonly #frames = new Map<number, Frame>();
  /** Frames being placed, so notes that arrive together share one. */
  readonly #placing = new Map<number, Promise<Frame | null>>();

  constructor(deps: NoteCenterDeps) {
    this.#deps = deps;
  }

  /** Gives `note` to the tab the person is looking at, when this browser can; says nothing otherwise. */
  async show(note: BrowserNote): Promise<void> {
    const tabId = await this.#deps
      .focusedTab()
      .catch(fallback('notes', 'Could not tell which tab the person is looking at', null));
    if (tabId === null) return;
    const frame = await this.#frame(tabId);
    if (frame === null) return;
    frame.sent.add(note.id);
    frame.last = note.id;
    if (frame.port) frame.port.postMessage({ type: 'note', note });
    else frame.waiting.push({ note, at: this.#deps.now() });
  }

  #frame(tabId: number): Promise<Frame | null> {
    const frame = this.#frames.get(tabId);
    if (frame) return Promise.resolve(frame);
    let placing = this.#placing.get(tabId);
    if (!placing) {
      placing = this.#deps
        .mount(tabId)
        .catch(fallback('notes', 'The page could not take the notes frame', null))
        .then((token) => {
          this.#placing.delete(tabId);
          if (token === null) return null;
          const placed: Frame = { token, port: null, waiting: [], sent: new Set(), last: null };
          this.#frames.set(tabId, placed);
          setTimeout(() => {
            if (this.#frames.get(tabId) === placed && placed.port === null) this.#close(tabId);
          }, CONNECT_WAIT_MS);
          return placed;
        });
      this.#placing.set(tabId, placing);
    }
    return placing;
  }

  /** A port opened to the background: a notes page's when its first message carries a frame's token. */
  connect(port: NotesPort): void {
    if (port.name !== NOTES_PORT) return;
    let tabId: number | null = null;
    port.onMessage.addListener((message) => {
      if (!isFromNotesPage(message)) {
        log.warn('notes', 'The notes page sent a message it should not have');
        return;
      }
      if (message.type === 'hello') {
        tabId = this.#adopt(port, message.token);
        if (tabId === null) port.disconnect();
        return;
      }
      if (tabId !== null) this.#fromPage(tabId, port, message);
    });
    port.onDisconnect.addListener(() => {
      if (tabId !== null && this.#frames.get(tabId)?.port === port) this.#frames.delete(tabId);
    });
  }

  /** What the proved notes page of tab `tabId` says over `port`, while that is still its frame's. */
  #fromPage(
    tabId: number,
    port: NotesPort,
    message: Exclude<FromNotesPage, { type: 'hello' }>,
  ): void {
    const frame = this.#frames.get(tabId);
    if (frame?.port !== port) return;
    switch (message.type) {
      case 'shown':
        if (frame.sent.has(message.id)) {
          void this.#deps
            .shown(message.id)
            .catch(fallback('notes', 'Could not tell the app a note showed', undefined));
        }
        return;
      case 'pressed':
        if (frame.sent.has(message.id)) {
          void this.#deps
            .pressed(message.id, message.button)
            .catch(fallback('notes', 'Could not tell the app a note was pressed', undefined));
        }
        return;
      case 'empty':
        if (message.last === frame.last) this.#close(tabId);
        return;
    }
  }

  /** Gives the frame whose token this is its port and its waiting notes; its tab, or null for no such frame. */
  #adopt(port: NotesPort, token: string): number | null {
    for (const [tabId, frame] of this.#frames) {
      if (frame.token !== token || frame.port !== null) continue;
      frame.port = port;
      // A note that waited longer than the app does was told in a system notification by now.
      const since = this.#deps.now() - NOTE_ANSWER_WAIT_MS;
      for (const { note, at } of frame.waiting.splice(0)) {
        if (at >= since) port.postMessage({ type: 'note', note });
      }
      return tabId;
    }
    return null;
  }

  /** The tab's frame has no notes left, or never connected: it goes. */
  #close(tabId: number): void {
    const frame = this.#frames.get(tabId);
    this.#frames.delete(tabId);
    frame?.port?.disconnect();
    void this.#deps
      .unmount(tabId)
      .catch(fallback('notes', 'Could not take the notes frame away', undefined));
  }

  /** The tab closed: its frame went with it. */
  tabRemoved(tabId: number): void {
    this.#frames.delete(tabId);
  }
}
