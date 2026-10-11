// Turning raw numbers from the backend into short, readable text.

import type {
  Category,
  DownloadSummary,
  DownloadStatus,
  FixAction,
  MediaKind,
  Schedule,
  TimeOfDay,
  WaitReason,
  Weekday,
} from '@wirecatch/protocol';

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

/** Bytes as a short size, for example `4.7 GB`. Uses 1024 steps, like most file managers. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '0 B';
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  if (unit === 0) return `${Math.round(value)} B`;
  const digits = value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(digits)} ${UNITS[unit]}`;
}

/** Bytes per second as a speed, for example `12.4 MB/s`. Zero speed is an empty string. */
export function formatSpeed(bytesPerSecond: number): string {
  if (bytesPerSecond <= 0) return '';
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** A limit or setting value: `4 MB/s`, `1.5 MB/s`, `512 KB/s`, without trailing zeros. */
export function formatRate(bytesPerSecond: number): string {
  let value = bytesPerSecond;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const rounded = Math.round(value * 10) / 10;
  return `${rounded} ${UNITS[unit]}/s`;
}

/** Seconds left as a compact duration: `45 s`, `3 min`, `1 h 20 min`, `2 days`. */
export function formatEta(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds) || seconds < 0) return '';
  if (seconds < 60) return `${Math.max(1, Math.round(seconds))} s`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) {
    const rest = minutes % 60;
    return rest === 0 ? `${hours} h` : `${hours} h ${rest} min`;
  }
  return `${Math.round(hours / 24)} days`;
}

/** Progress as a fraction between 0 and 1, or `null` when the total size is not known. */
export function progressFraction(done: number, total: number | null): number | null {
  if (total === null || total <= 0) return null;
  return Math.min(1, Math.max(0, done / total));
}

/** Progress as a whole percentage string, for example `42%`. Empty when unknown. */
export function formatPercent(done: number, total: number | null): string {
  const f = progressFraction(done, total);
  if (f === null) return '';
  // Never show 100% before the download has actually finished.
  const pct = f >= 1 ? 100 : Math.min(99, Math.floor(f * 100));
  return `${pct}%`;
}

/** Size column text: `1.2 GB of 4.7 GB`, `4.7 GB`, or what is known so far. */
export function formatSize(done: number, total: number | null, status: DownloadStatus): string {
  if (total === null) return done > 0 ? formatBytes(done) : 'Size not known yet';
  if (status === 'completed' || done >= total) return formatBytes(total);
  if (done === 0) return formatBytes(total);
  return `${formatBytes(done)} of ${formatBytes(total)}`;
}

export const STATUS_LABEL: Record<DownloadStatus, string> = {
  queued: 'Waiting',
  connecting: 'Connecting',
  downloading: 'Downloading',
  paused: 'Paused',
  completed: 'Finished',
  failed: 'Needs attention',
  cancelled: 'Cancelled',
  processing: 'Joining video and audio',
};

/** Where a media download came from, in plain words. */
export const MEDIA_SOURCE_LABEL: Record<MediaKind, string> = {
  hls: 'from a video stream',
  dash: 'from a video stream',
  direct: 'a media file',
  page: 'from a video page',
};

export const CATEGORY_LABEL: Record<Category, string> = {
  video: 'Video',
  audio: 'Music and audio',
  archive: 'Archives',
  document: 'Documents',
  program: 'Programs',
  image: 'Images',
  other: 'Other files',
};

export const CATEGORIES: readonly Category[] = [
  'video',
  'audio',
  'archive',
  'document',
  'program',
  'image',
  'other',
];

export const WEEKDAYS: readonly Weekday[] = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];

export const WEEKDAY_SHORT: Record<Weekday, string> = {
  mon: 'Mon',
  tue: 'Tue',
  wed: 'Wed',
  thu: 'Thu',
  fri: 'Fri',
  sat: 'Sat',
  sun: 'Sun',
};

/** `07:05` style, 24 hour clock. */
export function formatTime(t: TimeOfDay): string {
  return `${String(t.hour).padStart(2, '0')}:${String(t.minute).padStart(2, '0')}`;
}

/** The clock time of a Unix time in milliseconds, `07:05` style, in local time. */
export function clockTime(ms: number): string {
  const d = new Date(ms);
  return formatTime({ hour: d.getHours(), minute: d.getMinutes() });
}

/** Parses `HH:MM`. Returns `null` for anything that is not a valid time. */
export function parseTime(text: string): TimeOfDay | null {
  const m = /^\s*(\d{1,2}):(\d{2})\s*$/.exec(text);
  if (!m) return null;
  const hour = Number(m[1]);
  const minute = Number(m[2]);
  if (hour > 23 || minute > 59) return null;
  return { hour, minute };
}

/** Day list as words: `Every day`, `Weekdays`, `Weekends`, or `Mon, Wed, Fri`. */
export function formatDays(days: readonly Weekday[]): string {
  const set = new Set(days);
  if (set.size === 0 || set.size === 7) return 'Every day';
  const weekdays: Weekday[] = ['mon', 'tue', 'wed', 'thu', 'fri'];
  if (set.size === 5 && weekdays.every((d) => set.has(d))) return 'Weekdays';
  if (set.size === 2 && set.has('sat') && set.has('sun')) return 'Weekends';
  return WEEKDAYS.filter((d) => set.has(d))
    .map((d) => WEEKDAY_SHORT[d])
    .join(', ');
}

/** A schedule in one short phrase, for example `01:00 to 07:00, weekdays`. */
export function formatSchedule(schedule: Schedule | null): string {
  if (!schedule) return 'Runs any time';
  const window = schedule.stop
    ? `${formatTime(schedule.start)} to ${formatTime(schedule.stop)}`
    : `From ${formatTime(schedule.start)} until empty`;
  const days = formatDays(schedule.days);
  return days === 'Every day' ? window : `${window}, ${days.toLowerCase()}`;
}

/** Whether a schedule allows running at `now` (local time). No schedule means always. */
export function scheduleAllows(schedule: Schedule | null, now: Date): boolean {
  if (!schedule) return true;
  const today = WEEKDAYS[(now.getDay() + 6) % 7] as Weekday;
  const minutes = now.getHours() * 60 + now.getMinutes();
  const start = schedule.start.hour * 60 + schedule.start.minute;
  const dayOk = (d: Weekday) => schedule.days.length === 0 || schedule.days.includes(d);
  if (!schedule.stop) return dayOk(today) && minutes >= start;
  const stop = schedule.stop.hour * 60 + schedule.stop.minute;
  if (start <= stop) return dayOk(today) && minutes >= start && minutes < stop;
  // The window crosses midnight: the part after midnight belongs to the previous day.
  const yesterday = WEEKDAYS[(now.getDay() + 5) % 7] as Weekday;
  return (dayOk(today) && minutes >= start) || (dayOk(yesterday) && minutes < stop);
}

/** Speed limit text for a queue or the whole app. */
export function formatLimit(bytesPerSecond: number | null): string {
  return bytesPerSecond === null ? 'No speed limit' : `Limit ${formatRate(bytesPerSecond)}`;
}

/** Host name of a URL, or the URL itself if it does not parse. */
/**
 * A link as it may be shown or copied: a password in it becomes `***` (`https://ada:***@host/`),
 * and a user name without a password too, since some sites put a token there. The core already
 * sends links this way; this keeps the UI safe whatever it is given.
 */
export function linkForDisplay(url: string): string {
  const scheme = url.indexOf('://');
  if (scheme < 0) return url;
  const start = scheme + 3;
  const rest = url.slice(start);
  const end = rest.search(/[/?#]/);
  const authority = end < 0 ? rest : rest.slice(0, end);
  const at = authority.lastIndexOf('@');
  if (at < 0) return url;
  const userinfo = authority.slice(0, at);
  const colon = userinfo.indexOf(':');
  const masked = colon < 0 ? '***' : `${userinfo.slice(0, colon)}:***`;
  return `${url.slice(0, start)}${masked}${rest.slice(at)}`;
}

export function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/** A date for the detail panel, for example `Today, 14:05` or `3 Oct 2026, 09:12`. */
export function formatDate(ms: number, now: Date = new Date()): string {
  const d = new Date(ms);
  const time = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  const sameDay = (a: Date, b: Date) =>
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate();
  if (sameDay(d, now)) return `Today, ${time}`;
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (sameDay(d, yesterday)) return `Yesterday, ${time}`;
  const month = d.toLocaleString('en-GB', { month: 'short' });
  return `${d.getDate()} ${month} ${d.getFullYear()}, ${time}`;
}

/** Pulls every http, https or ftp link out of pasted text, without duplicates. */
export function extractUrls(text: string): string[] {
  const found = text.match(/\b(?:https?|ftps?):\/\/[^\s<>"'`]+/gi) ?? [];
  const out: string[] = [];
  for (const raw of found) {
    const cleaned = raw.replace(/[),.;:!?\]]+$/, '');
    try {
      const url = new URL(cleaned);
      if (!out.includes(url.href)) out.push(url.href);
    } catch {
      // Not a valid URL after all; skip it.
    }
  }
  return out;
}

/** The last path segment of a URL, decoded, as a guess at the file name. */
export function fileNameFromUrl(url: string): string {
  try {
    const u = new URL(url);
    const last = u.pathname.split('/').filter(Boolean).pop();
    if (!last) return u.host;
    return decodeURIComponent(last);
  } catch {
    return url;
  }
}

/** The folder part of a full file path, for either separator. */
export function folderOf(path: string): string {
  const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return i > 0 ? path.slice(0, i) : path;
}

/** Shows a portable path like `{Downloads}/Video` as `Downloads/Video`. */
export function displayPath(path: string): string {
  return path.replace(/^\{(\w+)\}/, '$1');
}

/** The button text for each fix an error can offer. */
export const FIX_LABEL: Record<FixAction, string> = {
  retry: 'Try again',
  refresh_link: 'Get a fresh link',
  choose_folder: 'Choose another folder',
  free_disk_space: 'Open the folder to free space',
};

/** Shorter button text for the fix inside a list row, where room is tight. */
export const FIX_SHORT: Record<FixAction, string> = {
  retry: 'Try again',
  refresh_link: 'New link',
  choose_folder: 'Change folder',
  free_disk_space: 'Open folder',
};

/** 1st, 2nd, 3rd, 4th, 11th, 21st. */
export function ordinal(n: number): string {
  const tens = n % 100;
  if (tens >= 11 && tens <= 13) return `${n}th`;
  const suffix = ['th', 'st', 'nd', 'rd'][n % 10] ?? 'th';
  return `${n}${n % 10 > 3 ? 'th' : suffix}`;
}

/** Why a download waits, in a couple of words for its row. */
export function waitShort(w: WaitReason): string {
  switch (w.type) {
    case 'queue_paused':
      return 'Queue paused';
    case 'queue_schedule':
      return w.data.starts_at ? `Starts ${formatTime(w.data.starts_at)}` : 'Outside its hours';
    case 'queue_full':
      return 'Queue full';
    case 'global_limit':
      return 'Overall limit';
    case 'in_line':
      return `${ordinal(w.data.position)} in line`;
    case 'server_refused':
      return `Retrying at ${clockTime(w.data.retry_at)}`;
  }
}

/**
 * The wait reason a row shows: why a queued download waits, or that a video's server refused it
 * for now and when the next try is (on a download that is connecting). `null` otherwise.
 */
export function shownWait(row: Pick<DownloadSummary, 'status' | 'waiting'>): WaitReason | null {
  const w = row.waiting;
  if (!w) return null;
  if (row.status === 'queued') return w;
  return w.type === 'server_refused' && row.status === 'connecting' ? w : null;
}

/** A download's status in a couple of words, as its list row shows it. */
export function statusShort(row: Pick<DownloadSummary, 'status' | 'waiting'>): string {
  const wait = shownWait(row);
  return wait ? waitShort(wait) : STATUS_LABEL[row.status];
}

/**
 * What a download is doing, in a sentence, when there is more to say than its status: why it
 * waits, or that its video and audio are being joined. `null` otherwise.
 */
export function statusDetail(row: DownloadSummary, queueName: string): string | null {
  const wait = shownWait(row);
  if (wait) return waitLong(wait, queueName);
  if (row.status === 'processing') {
    return 'The video and audio are downloaded; Wirecatch is joining them into one file.';
  }
  return null;
}

/** "today at 18:38", "yesterday at 09:05" or "on 3 Oct 2026". */
function whenSaid(ms: number, now: Date): string {
  const date = formatDate(ms, now);
  if (date.startsWith('Today, ')) return `today at ${date.slice(7)}`;
  if (date.startsWith('Yesterday, ')) return `yesterday at ${date.slice(11)}`;
  return `on ${date.slice(0, date.lastIndexOf(','))}`;
}

/**
 * One line for a download window whose download is already in the list (D23), for example
 * "You downloaded this today at 18:38." or "This is already downloading."
 */
export function earlierNote(row: DownloadSummary, now: Date = new Date()): string {
  switch (row.status) {
    case 'completed':
      return `You downloaded this ${whenSaid(row.completed_at ?? row.added_at, now)}.`;
    case 'connecting':
    case 'downloading':
    case 'processing':
      return 'This is already downloading.';
    case 'queued':
      return 'This is already in your list, waiting to start.';
    case 'paused':
      return 'This is already in your list, paused.';
    case 'failed':
      return 'This is already in your list and needs attention there.';
    case 'cancelled':
      return `You started this ${whenSaid(row.added_at, now)} and cancelled it.`;
  }
}

/** Why a download waits, in a full sentence for the detail panel. */
export function waitLong(w: WaitReason, queueName: string): string {
  switch (w.type) {
    case 'queue_paused':
      return `${queueName} is paused. Resume the queue to start it.`;
    case 'queue_schedule':
      return w.data.starts_at
        ? `Waiting for ${queueName}, which starts at ${formatTime(w.data.starts_at)}.`
        : `${queueName} is outside its hours.`;
    case 'queue_full':
      return `${queueName} is running ${w.data.running} of ${w.data.limit}. This starts when one of them finishes.`;
    case 'global_limit':
      return `Overall limit: ${w.data.running} of ${w.data.limit} downloads are running. This starts when one of them finishes.`;
    case 'in_line':
      return `${ordinal(w.data.position)} in line in ${queueName}.`;
    case 'server_refused':
      return `The video's server refused it for now, which sites sometimes do for a few minutes. Wirecatch tries again by itself at ${clockTime(w.data.retry_at)} and keeps what was already downloaded.`;
  }
}
