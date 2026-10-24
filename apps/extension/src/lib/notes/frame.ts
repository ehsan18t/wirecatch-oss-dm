/**
 * The notes frame in a web page (D65): the extension's notes page in a frame at the bottom right,
 * inside a closed shadow root so the website cannot reach the frame or read its address, which
 * carries the token the notes page proves itself with. The shadow root's host is placed once, when
 * the page script starts, and stays: the frame comes and goes inside it, where the website's own
 * observers cannot see, so the page's elements do not change when a note shows. The frame is only
 * as big as the notes page says its notes are, so the rest of the page stays clickable, and the
 * size comes over a channel of the frame's own, handed to the notes page alone.
 */
import { isFrameSize, SIZE_CHANNEL, type SizeChannel } from './messages';

/** Above anything a page can stack. */
const TOP_LAYER = '2147483647';

/** What the frame needs from the page around it. */
export interface FramePage {
  /** Where the host goes: the element shown full screen, else the document's root. */
  container: () => Element;
  /** The notes page's address with `token` after the `#`. */
  notesUrl: (token: string) => string;
  /** The extension's origin, the only one the size channel is handed to. */
  extensionOrigin: string;
  /** A fresh token no website can guess. */
  newToken: () => string;
}

export class NotesFrame {
  readonly #page: FramePage;
  readonly #host = document.createElement('div');
  readonly #shadow: ShadowRoot;
  #iframe: HTMLIFrameElement | null = null;
  #channel: MessageChannel | null = null;

  /** Places the empty host, which stays for as long as the page script runs. */
  constructor(page: FramePage) {
    this.#page = page;
    this.#shadow = this.#host.attachShadow({ mode: 'closed' });
    this.#host.style.setProperty('all', 'initial', 'important');
    this.place();
  }

  /** Puts the host where it shows: inside the element shown full screen, else the document's root. Called when the page goes in or out of full screen, which says nothing about notes. */
  place(): void {
    const container = this.#page.container();
    if (this.#host.parentElement !== container) container.append(this.#host);
  }

  /**
   * Places a fresh frame inside the host, taking away one already there, and gives the token its
   * notes page proves itself with. The background asks only when it has no frame for the tab, as
   * after it restarted, when the old frame's notes page lost its way to it. Null when the website
   * took the host away: putting it back now would show the website a note had come, so this page
   * takes no notes, and the app tells them another way.
   */
  mount(): string | null {
    this.unmount();
    if (!this.#host.isConnected) return null;
    const token = this.#page.newToken();
    const iframe = document.createElement('iframe');
    iframe.src = this.#page.notesUrl(token);
    iframe.title = 'Wirecatch notes';
    iframe.setAttribute('allowtransparency', 'true');
    // A frame whose colour scheme differs from the page around it gets an opaque backdrop; both say the same.
    Object.assign(iframe.style, {
      all: 'initial',
      position: 'fixed',
      right: '0',
      bottom: '0',
      width: '0',
      maxWidth: '100vw',
      height: '0',
      border: '0',
      colorScheme: 'light dark',
      zIndex: TOP_LAYER,
    });
    const channel = new MessageChannel();
    channel.port1.onmessage = (event: MessageEvent) => {
      if (!isFrameSize(event.data)) return;
      iframe.style.width = `${String(Math.ceil(event.data.wirecatchNotesWidth))}px`;
      iframe.style.height = `${String(Math.ceil(event.data.wirecatchNotesHeight))}px`;
    };
    // Handed over once the notes page has loaded, only to the extension's origin, and naming the
    // token, so the notes page takes no channel a website might post it.
    iframe.addEventListener('load', () => {
      const handOver: SizeChannel = { type: SIZE_CHANNEL, token };
      iframe.contentWindow?.postMessage(handOver, this.#page.extensionOrigin, [channel.port2]);
    });
    this.#shadow.append(iframe);
    this.#iframe = iframe;
    this.#channel = channel;
    return token;
  }

  /** Takes the frame away; the host stays. */
  unmount(): void {
    this.#channel?.port1.close();
    this.#channel = null;
    this.#iframe?.remove();
    this.#iframe = null;
  }

  /** Takes the host away too, for a page script that stops. */
  destroy(): void {
    this.unmount();
    this.#host.remove();
  }
}
