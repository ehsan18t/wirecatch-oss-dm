/**
 * Messages that carry the app's notes (D65) between the background, the page script that places
 * the notes frame in a tab, and the notes page inside that frame. The notes page talks to the
 * background only, over a port it proves with the token the page script gave it; the website
 * around the frame never sees a note or a token.
 */
import { isBrowserNote, isNoteId, type BrowserNote, type NoteId } from '@wirecatch/protocol';
import { fields, isNumber, isString, literal, refine, variants } from '../validate';

/** Asks a tab's page script to place the notes frame; it answers with a {@link MountAnswer}. */
export const NOTE_MOUNT = 'wirecatch-note-mount';
/** Asks a tab's page script to take the notes frame away: it has no notes left. */
export const NOTE_UNMOUNT = 'wirecatch-note-unmount';
/** Name of the port the notes page opens to the background. */
export const NOTES_PORT = 'wirecatch-notes';
/** The path of the notes page, as `runtime.getURL` takes it. */
export const NOTES_PAGE = '/notes.html';
/** What the page script posts the notes page, with the port its sizes go back on. */
export const SIZE_CHANNEL = 'wirecatch-notes-size';

/** The page script's hand-over of the size channel, naming the frame's token, which a website never sees. */
export interface SizeChannel {
  type: typeof SIZE_CHANNEL;
  token: string;
}

export const isSizeChannel = fields<SizeChannel>({ type: literal(SIZE_CHANNEL), token: isString });

export interface MountMessage {
  type: typeof NOTE_MOUNT;
}

export interface UnmountMessage {
  type: typeof NOTE_UNMOUNT;
}

/** The token the notes frame's page proves itself with. */
export interface MountAnswer {
  token: string;
}

export const isMountAnswer = fields<MountAnswer>({ token: isString });

/** What the notes page sends the background. */
export type FromNotesPage =
  /** First, the token its address carries. */
  | { type: 'hello'; token: string }
  /** The note is on screen. */
  | { type: 'shown'; id: NoteId }
  /** The person pressed one of a note's buttons. */
  | { type: 'pressed'; id: NoteId; button: number }
  /** Every note is gone, the last it was given `last`: the frame may go too, unless a newer note is on its way. */
  | { type: 'empty'; last: NoteId };

export const isFromNotesPage = variants<FromNotesPage>({
  hello: fields({ type: literal('hello'), token: isString }),
  pressed: fields({
    type: literal('pressed'),
    id: isNoteId,
    button: refine(isNumber, (n) => Number.isSafeInteger(n) && n >= 0),
  }),
  shown: fields({ type: literal('shown'), id: isNoteId }),
  empty: fields({ type: literal('empty'), last: isNoteId }),
});

/** What the background sends the notes page: a note to show. */
export interface ToNotesPage {
  type: 'note';
  note: BrowserNote;
}

export const isToNotesPage = fields<ToNotesPage>({ type: literal('note'), note: isBrowserNote });

/** What the notes page tells the page script around it, over the channel that script gave it: how big its frame must be, in CSS pixels. */
export interface FrameSize {
  wirecatchNotesWidth: number;
  wirecatchNotesHeight: number;
}

const isLength = refine(isNumber, (n) => Number.isFinite(n) && n >= 0);

export const isFrameSize = fields<FrameSize>({
  wirecatchNotesWidth: isLength,
  wirecatchNotesHeight: isLength,
});
