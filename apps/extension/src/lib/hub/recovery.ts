/** Settling a handover that a previous run of the background script started and never finished. */
import type { WithdrawOutcome } from '@wirecatch/protocol';
import {
  removeBrowserCopy,
  resumeBrowserCopy,
  type CopyRemoval,
  type DownloadControl,
} from '../capture/handover';
import { baseName } from '../file-types';
import { nonEmpty } from '../text';
import type { InFlightEntry } from './deps';

/** What became of a download recovered after a restart. */
export type Recovered =
  /** The person cancelled the browser copy or took it off the list: whatever the app has is the only copy. */
  | 'no_browser_copy'
  /** The app has it, and the browser copy is gone. */
  | 'handed_over'
  /** The app has it, but the browser copy could not be cancelled and stays paused in its list. */
  | 'copy_left'
  /** The app has it, and so does the browser: the person resumed its copy, or it finished. */
  | 'both'
  /** The app does not have it, and the browser copy runs (resumed here or by the person). */
  | 'browser_only'
  /** The app could not say whether it has it, and the browser copy runs too. */
  | 'uncertain'
  /** The browser would not resume its copy, which is still paused. */
  | 'stuck_paused';

const REMOVAL_RESULTS = {
  removed: 'handed_over',
  left_paused: 'copy_left',
  kept_running: 'both',
} as const satisfies Record<CopyRemoval, Recovered>;

export interface RecoveryDeps {
  downloads: DownloadControl;
  /** `withdraw_download`, connecting (or starting the app) first; rejects without an answer it can read. */
  withdraw(handoverId: string): Promise<WithdrawOutcome>;
}

/**
 * Settles one handover exactly as after a timeout: `withdraw_download` decides, whatever the
 * person did with the browser copy since, because `add_download` may have reached the app
 * before the restart. `already_started` means the app has it, so a copy still paused goes;
 * `withdrawn` or `not_found` means it does not, so a paused copy resumes; no answer, or one
 * that cannot be read, leaves the browser copy running, because two copies beat none.
 */
export async function recoverHandover(
  { id, handoverId }: InFlightEntry,
  deps: RecoveryDeps,
): Promise<{ result: Recovered; name: string }> {
  const { downloads } = deps;
  const state = await downloads.state(id).catch(() => null);
  const name = nonEmpty(baseName(state?.filename ?? '')) ?? 'A download';
  if (state === null || state.state === 'interrupted') return { result: 'no_browser_copy', name };
  const outcome = await deps.withdraw(handoverId).catch(() => null);
  if (outcome === 'already_started') {
    return { result: REMOVAL_RESULTS[await removeBrowserCopy(downloads, id)], name };
  }
  if (state.state === 'in_progress' && state.paused) {
    const copy = await resumeBrowserCopy(downloads, id);
    if (copy === 'stuck_paused') return { result: 'stuck_paused', name };
    if (copy === 'gone') return { result: 'no_browser_copy', name };
  }
  return { result: outcome === null ? 'uncertain' : 'browser_only', name };
}
