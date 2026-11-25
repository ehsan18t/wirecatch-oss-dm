import type { ElementInfo } from './messages';
import { fallback } from '../diag/log';
import { elementInfo } from './page-media';

/** A "nothing to download" answer is asked again after this long (media may show up once playback starts). */
const RECHECK_MS = 3000;

/**
 * Whether the background knows something to download for each media element. Each
 * element is asked about once per source, without waiting: a yes is kept, a no is
 * asked again after `RECHECK_MS`.
 */
export class DownloadableChecks {
  readonly #check: (info: ElementInfo) => Promise<boolean>;
  readonly #now: () => number;
  /** Called when an answer comes in, so the button can show. */
  readonly #answered: () => void;
  readonly #known = new WeakMap<
    HTMLMediaElement,
    { src: string | null; show: boolean; at: number }
  >();
  readonly #asking = new WeakSet<HTMLMediaElement>();

  constructor(
    check: (info: ElementInfo) => Promise<boolean>,
    now: () => number,
    answered: () => void,
  ) {
    this.#check = check;
    this.#now = now;
    this.#answered = answered;
  }

  /** The answer known now (false until one comes); asks when it does not know yet. */
  allowed(el: HTMLMediaElement): boolean {
    const { src } = elementInfo(el);
    const known = this.#known.get(el);
    if (known?.src === src && (known.show || this.#now() - known.at < RECHECK_MS)) {
      return known.show;
    }
    if (!this.#asking.has(el)) {
      this.#asking.add(el);
      this.#check(elementInfo(el))
        .catch(fallback('page', 'Could not ask whether to offer this media', false))
        .then((show) => {
          this.#asking.delete(el);
          this.#known.set(el, { src, show, at: this.#now() });
          this.#answered();
        })
        .catch(
          fallback('page', 'Could not record whether to offer this media', undefined, 'error'),
        );
    }
    return known?.show ?? false;
  }
}
