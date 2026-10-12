// Running backend actions for a window: a failure becomes words the person can read, said in
// whatever way the window shows errors (a note in the main window, a line in a download window).

import type { DownloadId } from '@wirecatch/protocol';

/** Said when an error carries no words of its own. */
const UNKNOWN_FAILURE = 'Something went wrong. Try again.';

/** An error in words the person can read. */
export function errorMessage(err: unknown): string {
  if (err instanceof Error && err.message) return err.message;
  if (typeof err === 'string' && err) return err;
  return UNKNOWN_FAILURE;
}

/** Says what failed in a bulk action, once, instead of one note per download. */
export function bulkFailureMessage(
  verb: string,
  failed: number,
  total: number,
  first: string,
): string {
  if (total === 1) return first;
  return `Could not ${verb} ${failed} of ${total} downloads. ${first}`;
}

export class Runner {
  readonly #fail: (text: string) => void;

  /** `fail` shows what went wrong. */
  constructor(fail: (text: string) => void) {
    this.#fail = fail;
  }

  /** Runs an action; on failure says why and gives `undefined`. */
  async run<T>(action: () => Promise<T>): Promise<T | undefined> {
    try {
      return await action();
    } catch (err) {
      this.#fail(errorMessage(err));
      return undefined;
    }
  }

  /** Runs an action and says whether it worked; on failure says why. */
  async succeeds(action: () => Promise<unknown>): Promise<boolean> {
    const ok = await this.run(async () => {
      await action();
      return true;
    });
    return ok === true;
  }

  /**
   * Runs one action for many downloads at once and reports any failures in a single note.
   * Returns how many succeeded.
   */
  async bulk(
    ids: DownloadId[],
    verb: string,
    action: (id: DownloadId) => Promise<void>,
  ): Promise<number> {
    if (ids.length === 0) return 0;
    const results = await Promise.allSettled(ids.map((id) => action(id)));
    const failures = results.filter((r): r is PromiseRejectedResult => r.status === 'rejected');
    if (failures.length > 0) {
      this.#fail(
        bulkFailureMessage(verb, failures.length, ids.length, errorMessage(failures[0]?.reason)),
      );
    }
    return ids.length - failures.length;
  }
}
