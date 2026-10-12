/** The one notice the popup and the badge show, and which outcomes are worth one. */
import type { HandoverResult } from '../capture/handover';
import type { Notice } from '../messages';
import type { NoticeStore } from './deps';
import { DuplicateWatch } from './duplicates';
import {
  copyLeftInBrowser,
  keptInBrowser,
  mayDownloadTwice,
  mayDownloadTwiceAfterRestart,
  stuckPaused,
} from './notices';
import type { Recovered } from './recovery';

export interface NoticeBoardDeps {
  now(): number;
  /** Where the notice is kept across a background restart; without it, it lasts as long as this run. */
  store?: NoticeStore;
  /** Called after every change, to publish the new notice. */
  changed(): void;
}

/**
 * Holds the notice and decides what each handover outcome says. Also watches for downloads
 * that may be in both places (`DuplicateWatch`), since that is said through the notice too.
 */
export class NoticeBoard {
  readonly #deps: NoticeBoardDeps;
  readonly #duplicates = new DuplicateWatch((name) => this.warn(mayDownloadTwice(name)));
  #notice: Notice | null = null;
  /** Whether this run set or dismissed the notice; then the one stored by an earlier run is out of date. */
  #changed = false;

  constructor(deps: NoticeBoardDeps) {
    this.#deps = deps;
  }

  get current(): Notice | null {
    return this.#notice;
  }

  /** Shows the notice an earlier run stored, unless this run already set or dismissed one. */
  restore(stored: Notice | null): void {
    if (!this.#changed) this.#notice = stored;
  }

  warn(message: string): void {
    this.notify('warning', message);
  }

  error(message: string): void {
    this.notify('error', message);
  }

  notify(tone: Notice['tone'], message: string): void {
    this.#set({ tone, message, at: this.#deps.now() });
  }

  dismiss(): void {
    this.#set(null);
  }

  /** Tells the person what became of a browser download, when there is something to say. */
  handover(result: HandoverResult, name: string): void {
    if (result.kind === 'handed_over') {
      if (result.browserCopy === 'left_paused') this.warn(copyLeftInBrowser(name));
      if (result.browserCopy === 'kept_running') this.warn(mayDownloadTwice(name));
      return;
    }
    // The person cancelled it in the browser meanwhile, or it finished: nothing to say.
    if (result.browserCopy === 'gone') return;
    if (result.browserCopy === 'stuck_paused') {
      this.error(stuckPaused(name));
      return;
    }
    // The app took it after all: the duplicate warning says so instead.
    const { handoverId } = result;
    if (result.uncertain && handoverId !== null) {
      if (this.#duplicates.keptUncertain(handoverId, name)) return;
    }
    const message = keptInBrowser(result, name);
    if (message !== null) this.warn(message);
  }

  /** The browser kept a copy of something the app may have; returns true when the app already confirmed it (and warns). */
  keptUncertain(handoverId: string, name: string): boolean {
    return this.#duplicates.keptUncertain(handoverId, name);
  }

  /** The app confirmed a handover after this side gave up waiting. */
  confirmedLate(handoverId: string, name: string): void {
    this.#duplicates.confirmedLate(handoverId, name);
  }

  /** Tells the person what became of a download a restarted background settled. */
  recovered(result: Recovered, name: string, handoverId: string): void {
    if (result === 'copy_left') this.warn(copyLeftInBrowser(name));
    if (result === 'both') this.warn(mayDownloadTwice(name));
    if (result === 'stuck_paused') this.error(stuckPaused(name));
    if (result === 'uncertain' && !this.#duplicates.keptUncertain(handoverId, name)) {
      this.warn(mayDownloadTwiceAfterRestart(name));
    }
  }

  #set(notice: Notice | null): void {
    this.#notice = notice;
    this.#changed = true;
    // Session storage unavailable: the notice then lasts as long as this background script.
    this.#deps.store?.save(notice).catch(() => undefined);
    this.#deps.changed();
  }
}
