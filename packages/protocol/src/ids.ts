// The one place a plain number becomes an id. The protocol's ids are branded (`DownloadId`, `QueueId`, `QuestionId`, `NoticeId`, `ProxyServerId`), so one cannot pass for another; an id that comes from the app is already typed, or is checked with its generated guard (`isDownloadId`). These constructors are for a number known to be the id: the desktop's stand-in backend, and tests. Each checks the number with the same guard, so no id is made without the check.
import type { DownloadId } from './generated/DownloadId';
import type { NoticeId } from './generated/NoticeId';
import type { ProxyServerId } from './generated/ProxyServerId';
import type { QuestionId } from './generated/QuestionId';
import type { QueueId } from './generated/QueueId';
import {
  isDownloadId,
  isNoticeId,
  isProxyServerId,
  isQuestionId,
  isQueueId,
} from './generated/validators';

/** `value` checked by `isId`, or a `RangeError` naming `what` it is not. */
function checked<T extends number>(
  value: number,
  isId: (value: unknown) => value is T,
  what: string,
): T {
  if (!isId(value)) throw new RangeError(`${String(value)} is not a ${what}.`);
  return value;
}

/** `value` as a download id. */
export function downloadId(value: number): DownloadId {
  return checked(value, isDownloadId, 'download id');
}

/** `value` as a queue id. */
export function queueId(value: number): QueueId {
  return checked(value, isQueueId, 'queue id');
}

/** `value` as the id of a question the app asks about a browser download. */
export function questionId(value: number): QuestionId {
  return checked(value, isQuestionId, 'question id');
}

/** `value` as the id of a notice the app asks a browser to show (D65). */
export function noticeId(value: number): NoticeId {
  return checked(value, isNoticeId, 'notice id');
}

/** `value` as the id of a proxy server (D73). */
export function proxyServerId(value: number): ProxyServerId {
  return checked(value, isProxyServerId, 'proxy server id');
}
