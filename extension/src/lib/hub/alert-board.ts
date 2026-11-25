/** The alerts the popup and the badge show, and which outcomes are worth one. */
import type { DownloadState, HandoverResult } from '../capture/handover';
import { fallback } from '../diag/log';
import type { Alert } from '../popup-port';
import type { AlertStore } from './deps';
import { DuplicateWatch } from './duplicates';
import {
  copyLeftInBrowser,
  keptInBrowser,
  mayDownloadTwice,
  mayDownloadTwiceUncertain,
  stuckPaused,
} from './alerts';
import type { RecoveryResult } from './recovery';

/** Alerts that cannot be saved last as long as this background script. */
const NOT_SAVED = 'Could not save the alerts';

/**
 * Most alerts kept. The popup lists them all, and a person who has not opened it for this many
 * has stopped reading them; the oldest go first, so the list cannot grow without bound.
 */
export const MAX_ALERTS = 5;

export interface AlertBoardDeps {
  now: () => number;
  /** Where the alerts are kept across a background restart; without it, they last as long as this run. */
  store?: AlertStore;
  /** Called after every change, to publish the alerts. */
  changed: () => void;
  /** How the browser lists the download now: null when it no longer does, `unreadable` when it would not say. */
  readCopy?: (downloadId: number) => Promise<DownloadState | null | 'unreadable'>;
}

/** What a new alert is about: one browser download (a newer alert about it replaces this one), or nothing in particular. */
interface About {
  downloadId?: number | null;
  /** Cleared once the browser reports the download running or finished; also makes it urgent. */
  untilResumed?: boolean;
  /** Asks the person to act: kept over the others when the list is full. */
  urgent?: boolean;
}

/**
 * Holds the alerts, newest first, at most one per browser download, and decides what each
 * handover outcome says. Also watches for downloads that may be in both places
 * (`DuplicateWatch`), since that is said through a alert too.
 */
export class AlertBoard {
  readonly #deps: AlertBoardDeps;
  readonly #duplicates = new DuplicateWatch(({ name, downloadId }, certain) => {
    void this.#lateMatch(name, downloadId, certain);
  });
  #alerts: Alert[] = [];
  /** Alerts dismissed in this run, so loading those an earlier run stored does not bring them back. */
  readonly #dismissed = new Set<string>();
  /**
   * How many times each download was reported running, finished, interrupted or erased in this
   * run: stored alerts asking to resume one are done, and a state read before a move is stale.
   */
  readonly #moved = new Map<number, number>();
  #nextId = 1;

  constructor(deps: AlertBoardDeps) {
    this.#deps = deps;
  }

  get current(): readonly Alert[] {
    return this.#alerts;
  }

  /**
   * Adds the alerts an earlier run stored to those of this run, except the ones dismissed,
   * replaced or resolved since. The browser is asked again about each download a alert asks to
   * resume, since one resumed while no background ran was never reported. Every answer is in
   * before anything is filtered, so a dismiss or a move reported while the browser was asked
   * still counts.
   */
  async load(): Promise<void> {
    const stored =
      (await this.#deps.store
        ?.load()
        .catch(fallback('alerts', 'Could not read the saved alerts', []))) ?? [];
    const paused = await Promise.all(stored.map((old) => this.#stillStands(old)));
    const kept = stored.filter(
      (old, i) =>
        paused[i] === true &&
        !this.#dismissed.has(old.id) &&
        (!old.untilResumed || old.downloadId === null || !this.#moved.has(old.downloadId)) &&
        this.#alerts.every((now) => now.id !== old.id && !sameDownload(now, old)),
    );
    this.#alerts = newestFirst([...this.#alerts, ...kept]);
    if (kept.length !== stored.length) {
      this.#deps.store?.save(this.#alerts).catch(fallback('alerts', NOT_SAVED, undefined));
    }
  }

  /** Whether a stored alert still stands as far as the browser can say: one not asking to resume always does. */
  async #stillStands(alert: Alert): Promise<boolean> {
    if (!alert.untilResumed || alert.downloadId === null) return true;
    const state = await this.#readCopy(alert.downloadId);
    return state === 'unreadable' || isPaused(state);
  }

  async #readCopy(downloadId: number | null): Promise<DownloadState | null | 'unreadable'> {
    const read = this.#deps.readCopy;
    if (downloadId === null || read === undefined) return 'unreadable';
    return read(downloadId).catch(
      fallback('alerts', 'Could not read a browser download', 'unreadable' as const),
    );
  }

  /**
   * The app answered late a handover whose browser copy was kept. What that copy is now decides
   * what to say, since the person may have resumed or paused it since: one the browser lists as
   * paused only needs removing when the app surely has it, and otherwise keeps its alert to
   * resume it (or, without one, is warned about as possibly in both places); one no longer
   * listed or interrupted leaves a single copy; any other, including one the browser will not
   * describe, may be downloading in both places. A state read across a reported move is read
   * again, so what is said matches the copy as it is.
   */
  async #lateMatch(name: string, downloadId: number | null, certain: boolean): Promise<void> {
    let state: DownloadState | null | 'unreadable';
    let moves: number | undefined;
    do {
      moves = downloadId === null ? undefined : this.#moved.get(downloadId);
      state = await this.#readCopy(downloadId);
    } while (downloadId !== null && this.#moved.get(downloadId) !== moves);
    if (isPaused(state)) {
      if (certain) {
        this.warn(copyLeftInBrowser(name), { downloadId });
        return;
      }
      // Its alert to resume it already says to check Wirecatch's list first.
      if (this.#alerts.some((n) => n.downloadId === downloadId && n.untilResumed)) return;
    }
    if (state === null || (state !== 'unreadable' && state.state === 'interrupted')) return;
    const message = certain ? mayDownloadTwice(name) : mayDownloadTwiceUncertain(name);
    this.warn(message, { downloadId, urgent: true });
  }

  warn(message: string, about: About = {}): void {
    this.notify('warning', message, about);
  }

  error(message: string, about: About = {}): void {
    this.notify('error', message, about);
  }

  notify(tone: Alert['tone'], message: string, about: About = {}): void {
    const downloadId = about.downloadId ?? null;
    const at = this.#deps.now();
    const id =
      downloadId === null ? `n${String(at)}-${String(this.#nextId++)}` : `d${String(downloadId)}`;
    const alert: Alert = {
      id,
      downloadId,
      untilResumed: about.untilResumed ?? false,
      urgent: (about.urgent ?? false) || (about.untilResumed ?? false),
      tone,
      message,
      at,
    };
    this.#set(newestFirst([alert, ...this.#alerts.filter((n) => n.id !== id)]));
  }

  dismiss(id: string): void {
    this.#dismissed.add(id);
    this.#set(this.#alerts.filter((n) => n.id !== id));
  }

  /** The browser reports a download running or finished: a alert asking to resume it is done. */
  downloadMoved(downloadId: number): void {
    this.#moved.set(downloadId, (this.#moved.get(downloadId) ?? 0) + 1);
    const left = this.#alerts.filter((n) => n.downloadId !== downloadId || !n.untilResumed);
    if (left.length !== this.#alerts.length) this.#set(left);
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
   * it is also registered with the duplicate watch, so a late answer, before or after, is told
   * from the copy's state when it arrives (see `#lateMatch`).
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
    if (appMayHave) this.#duplicates.keptUncertain(uncertainHandover, { name, downloadId });
  }

  /** The browser has a copy of something the app may have too: warns, surely if the app already confirmed it late. */
  uncertain(handoverId: string, name: string, downloadId: number | null): void {
    if (!this.#duplicates.keptUncertain(handoverId, { name, downloadId })) {
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

  #set(alerts: Alert[]): void {
    this.#alerts = newestFirst(alerts);
    this.#deps.store?.save(this.#alerts).catch(fallback('alerts', NOT_SAVED, undefined));
    this.#deps.changed();
  }
}

function isPaused(state: DownloadState | null | 'unreadable'): boolean {
  return state !== null && state !== 'unreadable' && state.state === 'in_progress' && state.paused;
}

function sameDownload(a: Alert, b: Alert): boolean {
  return a.downloadId !== null && a.downloadId === b.downloadId;
}

/**
 * Newest first, at most `MAX_ALERTS`. A alert asking the person to act stays until it is
 * resolved, so the oldest of the others go first, and an urgent one only when nothing else is
 * left to drop.
 */
function newestFirst(alerts: Alert[]): Alert[] {
  const sorted = [...alerts].sort((a, b) => b.at - a.at);
  while (sorted.length > MAX_ALERTS) {
    const other = sorted.map((n) => n.urgent).lastIndexOf(false);
    sorted.splice(other === -1 ? sorted.length - 1 : other, 1);
  }
  return sorted;
}
