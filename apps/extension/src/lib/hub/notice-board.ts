/** The one notice the popup and the badge show, and which outcomes are worth one. */
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
  readonly #duplicates = new DuplicateWatch((name, certain) =>
    this.warn(certain ? mayDownloadTwice(name) : mayDownloadTwiceUncertain(name)),
  );
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

  /**
   * Shows a notice. There is room for one, so a warning never replaces an error: an error
   * asks the person to act (a download left paused), and hiding it would leave that undone.
   */
  notify(tone: Notice['tone'], message: string): void {
    if (tone === 'warning' && this.#notice?.tone === 'error') return;
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
    const { handoverId } = result;
    if (result.uncertain && handoverId !== null) {
      this.uncertain(handoverId, name);
      return;
    }
    const message = keptInBrowser(result, name);
    if (message !== null) this.warn(message);
  }

  /** The browser has a copy of something the app may have too: warns, surely if the app already confirmed it late. */
  uncertain(handoverId: string, name: string): void {
    if (!this.#duplicates.keptUncertain(handoverId, name))
      this.warn(mayDownloadTwiceUncertain(name));
  }

  confirmedLate(handoverId: string, name: string, certain: boolean): void {
    this.#duplicates.confirmedLate(handoverId, name, certain);
  }

  /** Tells the person what became of a download a restarted background settled. */
  recovered(result: Recovered, name: string): void {
    if (result === 'copy_left') this.warn(copyLeftInBrowser(name));
    if (result === 'both') this.warn(mayDownloadTwice(name));
    if (result === 'stuck_paused') this.error(stuckPaused(name));
    // Not registered with the duplicate watch: the ports of the run that sent it are gone, so
    // no late answer to it can come.
    if (result === 'uncertain') this.warn(mayDownloadTwiceUncertain(name));
  }

  #set(notice: Notice | null): void {
    this.#notice = notice;
    this.#changed = true;
    // Session storage unavailable: the notice then lasts as long as this background script.
    this.#deps.store?.save(notice).catch(() => undefined);
    this.#deps.changed();
  }
}
