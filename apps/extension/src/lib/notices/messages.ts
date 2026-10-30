/**
 * Messages that carry the app's notices (D65) between the background, the page script that places
 * the notices frame in a tab, and the notices page inside that frame. The notices page talks to the
 * background only, over a port it proves with the token the page script gave it; the website
 * around the frame never sees a notice or a token.
 */
import {
  isBrowserNotice,
  isNoticeId,
  type BrowserNotice,
  type NoticeId,
} from '@wirecatch/protocol';
import { fields, isNumber, isString, literal, refine, variants } from '../validate';

/** Asks a tab's page script to place the notices frame; it answers with a {@link MountAnswer}. */
export const NOTICE_MOUNT = 'wirecatch-notice-mount';
/** Asks a tab's page script to take the notices frame away: it has no notices left. */
export const NOTICE_UNMOUNT = 'wirecatch-notice-unmount';
/** Name of the port the notices page opens to the background. */
export const NOTICES_PORT = 'wirecatch-notices';
/** The path of the notices page, as `runtime.getURL` takes it. */
export const NOTICES_PAGE = '/notices.html';
/** What the page script posts the notices page, with the port its sizes go back on. */
export const SIZE_CHANNEL = 'wirecatch-notices-size';

/** The page script's hand-over of the size channel, naming the frame's token, which a website never sees. */
export interface SizeChannel {
  type: typeof SIZE_CHANNEL;
  token: string;
}

export const isSizeChannel = fields<SizeChannel>({ type: literal(SIZE_CHANNEL), token: isString });

export interface MountMessage {
  type: typeof NOTICE_MOUNT;
}

export interface UnmountMessage {
  type: typeof NOTICE_UNMOUNT;
}

/** The token the notices frame's page proves itself with. */
export interface MountAnswer {
  token: string;
}

export const isMountAnswer = fields<MountAnswer>({ token: isString });

/** What the notices page sends the background. */
export type FromNoticesPage =
  /** First, the token its address carries. */
  | { type: 'hello'; token: string }
  /** The notice is on screen. */
  | { type: 'shown'; id: NoticeId }
  /** The person pressed one of a notice's buttons. */
  | { type: 'pressed'; id: NoticeId; button: number }
  /** The notice left the page: closed, pressed, cleared or its time was up. */
  | { type: 'gone'; id: NoticeId }
  /** Every notice is gone, the last it was given `last`: the frame may go too, unless a newer notice is on its way. */
  | { type: 'empty'; last: NoticeId };

export const isFromNoticesPage = variants<FromNoticesPage>({
  hello: fields({ type: literal('hello'), token: isString }),
  pressed: fields({
    type: literal('pressed'),
    id: isNoticeId,
    button: refine(isNumber, (n) => Number.isSafeInteger(n) && n >= 0),
  }),
  shown: fields({ type: literal('shown'), id: isNoticeId }),
  gone: fields({ type: literal('gone'), id: isNoticeId }),
  empty: fields({ type: literal('empty'), last: isNoticeId }),
});

/** What the background sends the notices page: a notice to show. */
export interface ToNoticesPage {
  type: 'notice';
  notice: BrowserNotice;
}

export const isToNoticesPage = fields<ToNoticesPage>({
  type: literal('notice'),
  notice: isBrowserNotice,
});

/** What the notices page tells the page script around it, over the channel that script gave it: how big its frame must be, in CSS pixels. */
export interface FrameSize {
  wirecatchNoticesWidth: number;
  wirecatchNoticesHeight: number;
}

const isLength = refine(isNumber, (n) => Number.isFinite(n) && n >= 0);

export const isFrameSize = fields<FrameSize>({
  wirecatchNoticesWidth: isLength,
  wirecatchNoticesHeight: isLength,
});
