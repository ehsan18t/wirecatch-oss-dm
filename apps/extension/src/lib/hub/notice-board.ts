/** The notices the popup and the badge show, and which outcomes are worth one. */
import type { HandoverResult } from '../capture/handover';
import type { Notice } from '../messages';
import type { NoticeStore } from './deps';
import { DuplicateWatch } from './duplicates';
import {
  copyLeftInBrowser,
  keptInBrowser,
  mayDownloadTwice,
  mayDownloadTwiceUncertain,
  stuckPaused,
} from './notices';
import type { RecoveryResult } from './recovery';

/**
 * Most notices kept. The popup lists them all, and a person who has not opened it for this many
 * has stopped reading them; the oldest go first, so the list cannot grow without bound.
 */
export const MAX_NOTICES = 5;

export interface NoticeBoardDeps {
  now(): number;
  /** Where the notices are kept across a background restart; without it, they last as long as this run. */
  store?: NoticeStore;
  /** Called after every change, to publish the notices. */
  changed(): void;
  /** Whether the browser still lists the download as paused (or cannot say). */
  stillPaused?(downloadId: number): Promise<boolean>;
}

/** What a new notice is about: one browser download (a newer notice about it replaces this one), or nothing in particular. */
interface About {
  downloadId?: number | null;
  /** Cleared once the browser reports the download running or finished; also makes it urgent. */
  untilResumed?: boolean;
  /** Asks the person to act: kept over the others when the list is full. */
  urgent?: boolean;
}

/**
 * Holds the notices, newest first, at most one per browser download, and decides what each
 * handover outcome says. Also watches for downloads that may be in both places
 * (`DuplicateWatch`), since that is said through a notice too.
 */
export class NoticeBoard {
  readonly #deps: NoticeBoardDeps;
  readonly #duplicates = new DuplicateWatch(({ name, downloadId, paused }, certain) => {
    if (!paused) {
      const message = certain ? mayDownloadTwice(name) : mayDownloadTwiceUncertain(name);
      this.warn(message, { downloadId, urgent: true });
    } else if (certain) {
      // The app has it: the paused copy only needs removing, not resuming.
      this.warn(copyLeftInBrowser(name), { downloadId });
    }
    // A paused copy and an unreadable answer: the notice to resume it already says to check
    // Wirecatch's list first, so it stays.
  });
  #notices: Notice[] = [];
  /** Notices dismissed in this run, so loading those an earlier run stored does not bring them back. */
  readonly #dismissed = new Set<string>();
  /** Downloads reported running or finished in this run, whose stored notices asking to resume them are done. */
  readonly #moved = new Set<number>();
  #nextId = 1;

  constructor(deps: NoticeBoardDeps) {
    this.#deps = deps;
  }

  get current(): readonly Notice[] {
    return this.#notices;
  }

  /**
   * Adds the notices an earlier run stored to those of this run, except the ones dismissed,
   * replaced or resolved since. The browser is asked again about each download a notice asks to
   * resume, since one resumed while no background ran was never reported. Every answer is in
   * before anything is filtered, so a dismiss or a move reported while the browser was asked
   * still counts.
   */
  async load(): Promise<void> {
    const stored = (await this.#deps.store?.load().catch(() => [])) ?? [];
    const paused = await Promise.all(stored.map((old) => this.#stillPaused(old)));
    const kept = stored.filter(
      (old, i) =>
        paused[i] === true &&
        !this.#dismissed.has(old.id) &&
        !(old.untilResumed && old.downloadId !== null && this.#moved.has(old.downloadId)) &&
        !this.#notices.some((now) => now.id === old.id || sameDownload(now, old)),
    );
    this.#notices = newestFirst([...this.#notices, ...kept]);
    if (kept.length !== stored.length) this.#deps.store?.save(this.#notices).catch(() => undefined);
  }

  /** Whether a stored notice still stands as far as the browser can say: one not asking to resume always does. */
  async #stillPaused(notice: Notice): Promise<boolean> {
    if (!notice.untilResumed || notice.downloadId === null) return true;
    return (await this.#deps.stillPaused?.(notice.downloadId).catch(() => true)) ?? true;
  }

  warn(message: string, about: About = {}): void {
    this.notify('warning', message, about);
  }

  error(message: string, about: About = {}): void {
    this.notify('error', message, about);
  }

  notify(tone: Notice['tone'], message: string, about: About = {}): void {
    const downloadId = about.downloadId ?? null;
    const at = this.#deps.now();
    const id =
      downloadId === null ? `n${String(at)}-${String(this.#nextId++)}` : `d${String(downloadId)}`;
    const notice: Notice = {
      id,
      downloadId,
      untilResumed: about.untilResumed ?? false,
      urgent: (about.urgent ?? false) || (about.untilResumed ?? false),
      tone,
      message,
      at,
    };
    this.#set(newestFirst([notice, ...this.#notices.filter((n) => n.id !== id)]));
  }

  dismiss(id: string): void {
    this.#dismissed.add(id);
    this.#set(this.#notices.filter((n) => n.id !== id));
  }

  /** The browser reports a download running or finished: a notice asking to resume it is done. */
  downloadMoved(downloadId: number): void {
    this.#moved.add(downloadId);
    const left = this.#notices.filter((n) => !(n.downloadId === downloadId && n.untilResumed));
    if (left.length !== this.#notices.length) this.#set(left);
  }

  /** Tells the person what became of a browser download, when there is something to say. */
  handover(result: HandoverResult, name: string, downloadId: number): void {
    const about = { downloadId };
    if (result.kind === 'handed_over') {
      if (result.browserCopy === 'left_paused') this.warn(copyLeftInBrowser(name), about);
      if (result.browserCopy === 'kept_running') {
        this.warn(mayDownloadTwice(name), { downloadId, urgent: true });
      }
      return;
    }
    if (result.browserCopy === 'gone') return;
    const { handoverId } = result;
    if (result.browserCopy === 'stuck_paused' || result.browserCopy === 'maybe_paused') {
      this.#notResumed(name, downloadId, result.browserCopy, result.uncertain ? handoverId : null);
      return;
    }
    if (result.uncertain && handoverId !== null) {
      this.uncertain(handoverId, name, downloadId);
      return;
    }
    const message = keptInBrowser(result, name);
    if (message !== null) this.warn(message, about);
  }

  /**
   * The browser would not resume its copy. With the handover id of one the app may have taken,
   * it is also registered as a paused copy, so a sure late confirmation, before or after, replaces
   * the notice to resume it with one saying the copy is left over.
   */
  #notResumed(
    name: string,
    downloadId: number,
    copy: 'stuck_paused' | 'maybe_paused',
    uncertainHandover: string | null,
  ): void {
    const appMayHave = uncertainHandover !== null;
    const certain = copy === 'stuck_paused';
    this.error(stuckPaused(name, { certain, appMayHave }), { downloadId, untilResumed: true });
    if (appMayHave)
      this.#duplicates.keptUncertain(uncertainHandover, { name, downloadId, paused: true });
  }

  /** The browser has a copy of something the app may have too: warns, surely if the app already confirmed it late. */
  uncertain(handoverId: string, name: string, downloadId: number | null): void {
    if (!this.#duplicates.keptUncertain(handoverId, { name, downloadId, paused: false })) {
      this.warn(mayDownloadTwiceUncertain(name), { downloadId, urgent: true });
    }
  }

  confirmedLate(handoverId: string, name: string, certain: boolean): void {
    this.#duplicates.confirmedLate(handoverId, name, certain);
  }

  /** Tells the person what became of a download a restarted background settled. */
  recovered({ result, name, appMayHave }: RecoveryResult, downloadId: number): void {
    const about = { downloadId };
    if (result === 'copy_left') this.warn(copyLeftInBrowser(name), about);
    if (result === 'both') this.warn(mayDownloadTwice(name), { downloadId, urgent: true });
    if (result === 'stuck_paused' || result === 'maybe_paused') {
      const certain = result === 'stuck_paused';
      this.error(stuckPaused(name, { certain, appMayHave }), { downloadId, untilResumed: true });
    }
    // Not registered with the duplicate watch: the ports of the run that sent it are gone, so
    // no late answer to it can come.
    if (result === 'uncertain') {
      this.warn(mayDownloadTwiceUncertain(name), { downloadId, urgent: true });
    }
  }

  #set(notices: Notice[]): void {
    this.#notices = newestFirst(notices);
    // Session storage unavailable: the notices then last as long as this background script.
    this.#deps.store?.save(this.#notices).catch(() => undefined);
    this.#deps.changed();
  }
}

function sameDownload(a: Notice, b: Notice): boolean {
  return a.downloadId !== null && a.downloadId === b.downloadId;
}

/**
 * Newest first, at most `MAX_NOTICES`. A notice asking the person to act stays until it is
 * resolved, so the oldest of the others go first, and an urgent one only when nothing else is
 * left to drop.
 */
function newestFirst(notices: Notice[]): Notice[] {
  const sorted = [...notices].sort((a, b) => b.at - a.at);
  while (sorted.length > MAX_NOTICES) {
    const other = sorted.map((n) => n.urgent).lastIndexOf(false);
    sorted.splice(other >= 0 ? other : sorted.length - 1, 1);
  }
  return sorted;
}
