/**
 * The notes frame in a web page (D65): the extension's notes page in a frame at the bottom right,
 * inside a closed shadow root so the website cannot reach the frame or read its address, which
 * carries the token the notes page proves itself with. The frame is only as big as the notes page
 * says its notes are, so the rest of the page stays clickable. The size comes over a channel of the
 * frame's own, handed to the notes page alone, so the website cannot tell when a note comes or goes.
 */
import { isFrameSize, SIZE_CHANNEL, type SizeChannel } from './messages';

/** Above anything a page can stack. */
const TOP_LAYER = '2147483647';

/** What the frame needs from the page around it. */
export interface FramePage {
  /** Where the frame goes: the element shown full screen, else the document's root. */
  container(): Element;
  /** The notes page's address with `token` after the `#`. */
  notesUrl(token: string): string;
  /** The extension's origin, the only one the size channel is handed to. */
  extensionOrigin: string;
  /** A fresh token no website can guess. */
  newToken(): string;
}

export class NotesFrame {
  readonly #page: FramePage;
  #host: HTMLElement | null = null;
  #channel: MessageChannel | null = null;

  constructor(page: FramePage) {
    this.#page = page;
  }

  /**
   * Places a fresh frame, taking away one already in place, and gives the token its notes page
   * proves itself with. The background asks only when it has no frame for the tab, as after it
   * restarted, when the old frame's notes page lost its way to it.
   */
  mount(): string {
    this.unmount();
    const token = this.#page.newToken();
    const host = document.createElement('div');
    const shadow = host.attachShadow({ mode: 'closed' });
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
    shadow.append(iframe);
    host.style.setProperty('all', 'initial', 'important');
    this.#page.container().append(host);
    this.#host = host;
    this.#channel = channel;
    return token;
  }

  /** Takes the frame away. */
  unmount(): void {
    this.#channel?.port1.close();
    this.#channel = null;
    this.#host?.remove();
    this.#host = null;
  }
}
