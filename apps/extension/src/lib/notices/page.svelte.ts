/**
 * The notices page's state (D65): the notices the background sends, as the design system's toasts,
 * each said to be on screen once the page script around has made the frame big enough to show it,
 * each button sent back to the background, each notice that leaves told to it so it never shows
 * again, and the frame's size for that page script. When the last notice is gone the background
 * takes the frame away.
 */
import type { BrowserNotice, NoticeId, NoticeKind } from '@wirecatch/protocol';
import { Toaster, type ToastTone } from '@wirecatch/ui';
import type { FrameSize, FromNoticesPage } from './messages';

const TONE: Record<NoticeKind, ToastTone> = {
  started: 'info',
  finished: 'success',
  failed: 'danger',
};

export class NoticesPage {
  readonly toaster = new Toaster();
  readonly #send: (message: FromNoticesPage) => void;
  /** The last notice given, which an `empty` names. */
  #last: NoticeId | null = null;
  /** Notices given but not yet in a frame big enough to show them. */
  readonly #unseen: NoticeId[] = [];
  /** Each toast's notice, until the toast leaves. */
  #notices: { toast: string; id: NoticeId }[] = [];
  /** Where sizes go, once the page script around handed it over. */
  #sizes: MessagePort | null = null;
  #size: FrameSize | null = null;

  constructor(send: (message: FromNoticesPage) => void) {
    this.#send = send;
  }

  /** Shows `notice` until its time is up, the person closes it, or presses one of its buttons. */
  show(notice: BrowserNotice): void {
    this.#last = notice.id;
    this.#unseen.push(notice.id);
    const toast = this.toaster.show(TONE[notice.kind], notice.body, {
      title: notice.title,
      duration: notice.stays_ms ?? 0,
      actions: notice.buttons.map((label, button) => ({
        label,
        run: () => this.#send({ type: 'pressed', id: notice.id, button }),
      })),
    });
    this.#notices.push({ toast, id: notice.id });
  }

  /** The app took notice `id` back. */
  withdraw(id: NoticeId): void {
    for (const notice of this.#notices) if (notice.id === id) this.toaster.dismiss(notice.toast);
  }

  /** The list changed: once every notice is gone, the frame may go. It reads the list first, so an effect calling it follows every change. */
  changed(): void {
    const { list } = this.toaster;
    const left = this.#notices.filter(({ toast }) => list.every((shown) => shown.id !== toast));
    this.#notices = this.#notices.filter((notice) => !left.includes(notice));
    for (const { id } of left) this.#send({ type: 'gone', id });
    const empty = list.length === 0;
    if (empty && this.#last !== null) this.#send({ type: 'empty', last: this.#last });
  }

  /** The port the page script around handed over for sizes; the last size goes at once. */
  sizesTo(port: MessagePort): void {
    if (this.#sizes !== null) return;
    this.#sizes = port;
    if (this.#size) this.#post(this.#size);
  }

  /** The notices now take this much room; none when there are none, so the frame shrinks away. */
  resized(size: FrameSize): void {
    const none = this.toaster.list.length === 0;
    this.#size = none ? { wirecatchNoticesWidth: 0, wirecatchNoticesHeight: 0 } : size;
    if (this.#sizes) this.#post(this.#size);
  }

  /** Sends `size` to the page script, and says the notices are on screen once the frame has room for them. */
  #post(size: FrameSize): void {
    this.#sizes?.postMessage(size);
    if (size.wirecatchNoticesHeight === 0) return;
    for (const id of this.#unseen.splice(0)) this.#send({ type: 'shown', id });
  }

  /** The background let this frame go: its notices go with it, still open for the tab the person turned to. */
  lost(): void {
    this.#notices = [];
    this.toaster.clear();
  }

  dispose(): void {
    this.toaster.dispose();
  }
}
