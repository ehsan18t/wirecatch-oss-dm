/**
 * The notes page's state (D65): the notes the background sends, as the design system's toasts,
 * each said to be on screen once the page script around has made the frame big enough to show it,
 * each button sent back to the background, and the frame's size for that page script. When the
 * last note is gone the background takes the frame away.
 */
import type { BrowserNote, NoteId, NoteKind } from '@wirecatch/protocol';
import { Toaster, type ToastTone } from '@wirecatch/ui';
import type { FrameSize, FromNotesPage } from './messages';

const TONE: Record<NoteKind, ToastTone> = {
  started: 'info',
  finished: 'success',
  failed: 'danger',
};

export class NotesPage {
  readonly toaster = new Toaster();
  readonly #send: (message: FromNotesPage) => void;
  /** The last note given, which an `empty` names. */
  #last: NoteId | null = null;
  /** Notes given but not yet in a frame big enough to show them. */
  readonly #unseen: NoteId[] = [];
  /** Where sizes go, once the page script around handed it over. */
  #sizes: MessagePort | null = null;
  #size: FrameSize | null = null;

  constructor(send: (message: FromNotesPage) => void) {
    this.#send = send;
  }

  /** Shows `note` until its time is up, the person closes it, or presses one of its buttons. */
  show(note: BrowserNote): void {
    this.#last = note.id;
    this.#unseen.push(note.id);
    this.toaster.show(TONE[note.kind], note.body, {
      title: note.title,
      duration: note.stays_ms ?? 0,
      actions: note.buttons.map((label, button) => ({
        label,
        run: () => this.#send({ type: 'pressed', id: note.id, button }),
      })),
    });
  }

  /** The list changed: once every note is gone, the frame may go. It reads the list first, so an effect calling it follows every change. */
  changed(): void {
    const empty = this.toaster.list.length === 0;
    if (empty && this.#last !== null) this.#send({ type: 'empty', last: this.#last });
  }

  /** The port the page script around handed over for sizes; the last size goes at once. */
  sizesTo(port: MessagePort): void {
    if (this.#sizes !== null) return;
    this.#sizes = port;
    if (this.#size) this.#post(this.#size);
  }

  /** The notes now take this much room; none when there are none, so the frame shrinks away. */
  resized(size: FrameSize): void {
    const none = this.toaster.list.length === 0;
    this.#size = none ? { wirecatchNotesWidth: 0, wirecatchNotesHeight: 0 } : size;
    if (this.#sizes) this.#post(this.#size);
  }

  /** Sends `size` to the page script, and says the notes are on screen once the frame has room for them. */
  #post(size: FrameSize): void {
    this.#sizes?.postMessage(size);
    if (size.wirecatchNotesHeight === 0) return;
    for (const id of this.#unseen.splice(0)) this.#send({ type: 'shown', id });
  }

  /** The background let this frame go: its notes go with it. */
  lost(): void {
    this.toaster.clear();
  }

  dispose(): void {
    this.toaster.dispose();
  }
}
