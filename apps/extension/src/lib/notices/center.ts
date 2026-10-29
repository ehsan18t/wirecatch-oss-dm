/**
 * The app's notices in the tab the person is looking at (D65). The app asks every connected browser;
 * this one shows a notice only while it is the program in front and its tab is a web page whose page
 * script can place the notices frame, and tells the app once the notices page has the notice on screen,
 * so the app shows a system notification whenever the notice did not show. Each tab's frame proves
 * itself with the token its page script made, so a website that frames the notices page itself gets
 * nothing.
 */
import { NOTICE_ANSWER_WAIT_MS, type BrowserNotice, type NoticeId } from '@wirecatch/protocol';
import { fallback, log } from '../diag/log';
import {
  isFromNoticesPage,
  NOTICES_PORT,
  type FromNoticesPage,
  type ToNoticesPage,
} from './messages';

/**
 * How long a frame has to connect before it is taken away: its page left, its website removed it,
 * or did not let it load. The app has told its notices another way by then.
 */
export const CONNECT_WAIT_MS = 3000;

/** What the notice center needs from the browser and the app. */
export interface NoticeCenterDeps {
  /** The tab the person is looking at, while the browser is in front and the tab is a web page; null otherwise. */
  focusedTab: () => Promise<number | null>;
  /** Asks the tab's page script to place a fresh notices frame: its token, or null when the page cannot. */
  mount: (tabId: number) => Promise<string | null>;
  /** Asks the tab's page script to take the notices frame away. */
  unmount: (tabId: number) => Promise<void>;
  /** Tells the app the notice is on screen. */
  shown: (id: NoticeId) => Promise<void>;
  /** Tells the app the person pressed one of the notice's buttons. */
  pressed: (id: NoticeId, button: number) => Promise<void>;
  /** Milliseconds since some fixed point. */
  now: () => number;
}

/** The part of a `runtime.Port` the notice center uses. */
export interface NoticesPort {
  name: string;
  postMessage: (message: ToNoticesPage) => void;
  disconnect: () => void;
  onMessage: { addListener: (listener: (message: unknown) => void) => void };
  onDisconnect: { addListener: (listener: () => void) => void };
}

/** One tab's notices frame. */
interface Frame {
  token: string;
  /** The notices page's port, once it has proved itself. */
  port: NoticesPort | null;
  /** Notices waiting for the page to connect, with when they came. */
  waiting: { notice: BrowserNotice; at: number }[];
  /** The notices sent to this tab, whose buttons it may press. */
  sent: Set<NoticeId>;
  /** The last notice sent, so an `empty` that crossed a newer notice does not close the frame. */
  last: NoticeId | null;
}

export class NoticeCenter {
  readonly #deps: NoticeCenterDeps;
  readonly #frames = new Map<number, Frame>();
  /** Frames being placed, so notices that arrive together share one. */
  readonly #placing = new Map<number, Promise<Frame | null>>();

  constructor(deps: NoticeCenterDeps) {
    this.#deps = deps;
  }

  /** Gives `notice` to the tab the person is looking at, when this browser can; says nothing otherwise. */
  async show(notice: BrowserNotice): Promise<void> {
    const tabId = await this.#deps
      .focusedTab()
      .catch(fallback('notices', 'Could not tell which tab the person is looking at', null));
    if (tabId === null) return;
    const frame = await this.#frame(tabId);
    if (frame === null) return;
    frame.sent.add(notice.id);
    frame.last = notice.id;
    if (frame.port) frame.port.postMessage({ type: 'notice', notice });
    else frame.waiting.push({ notice, at: this.#deps.now() });
  }

  #frame(tabId: number): Promise<Frame | null> {
    const frame = this.#frames.get(tabId);
    if (frame) return Promise.resolve(frame);
    let placing = this.#placing.get(tabId);
    if (!placing) {
      placing = this.#deps
        .mount(tabId)
        .catch(fallback('notices', 'The page could not take the notices frame', null))
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

  /** A port opened to the background: a notices page's when its first message carries a frame's token. */
  connect(port: NoticesPort): void {
    if (port.name !== NOTICES_PORT) return;
    let tabId: number | null = null;
    port.onMessage.addListener((message) => {
      if (!isFromNoticesPage(message)) {
        log.warn('notices', 'The notices page sent a message it should not have');
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

  /** What the proved notices page of tab `tabId` says over `port`, while that is still its frame's. */
  #fromPage(
    tabId: number,
    port: NoticesPort,
    message: Exclude<FromNoticesPage, { type: 'hello' }>,
  ): void {
    const frame = this.#frames.get(tabId);
    if (frame?.port !== port) return;
    switch (message.type) {
      case 'shown':
        if (frame.sent.has(message.id)) {
          void this.#deps
            .shown(message.id)
            .catch(fallback('notices', 'Could not tell the app a notice showed', undefined));
        }
        return;
      case 'pressed':
        if (frame.sent.has(message.id)) {
          void this.#deps
            .pressed(message.id, message.button)
            .catch(fallback('notices', 'Could not tell the app a notice was pressed', undefined));
        }
        return;
      case 'empty':
        if (message.last === frame.last) this.#close(tabId);
        return;
    }
  }

  /** Gives the frame whose token this is its port and its waiting notices; its tab, or null for no such frame. */
  #adopt(port: NoticesPort, token: string): number | null {
    for (const [tabId, frame] of this.#frames) {
      if (frame.token !== token || frame.port !== null) continue;
      frame.port = port;
      // A notice that waited longer than the app does was told in a system notification by now.
      const since = this.#deps.now() - NOTICE_ANSWER_WAIT_MS;
      for (const { notice, at } of frame.waiting.splice(0)) {
        if (at >= since) port.postMessage({ type: 'notice', notice });
      }
      return tabId;
    }
    return null;
  }

  /** The tab's frame has no notices left, or never connected: it goes. */
  #close(tabId: number): void {
    const frame = this.#frames.get(tabId);
    this.#frames.delete(tabId);
    frame?.port?.disconnect();
    void this.#deps
      .unmount(tabId)
      .catch(fallback('notices', 'Could not take the notices frame away', undefined));
  }

  /** The tab closed: its frame went with it. */
  tabRemoved(tabId: number): void {
    this.#frames.delete(tabId);
  }
}
