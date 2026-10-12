/** Settling a handover that a previous run of the background script started and never finished. */
import type { WithdrawOutcome } from '@wirecatch/protocol';
import { removeBrowserCopy, resumeBrowserCopy, type DownloadControl } from '../capture/handover';
import { baseName } from '../file-types';
import { nonEmpty } from '../text';
import type { InFlightEntry } from './deps';

/** What became of a download recovered after a restart. */
export type Recovered =
  /** No longer paused (finished, cancelled or resumed meanwhile): nothing waited on the handover. */
  | 'not_waiting'
  /** The app has it, and the browser copy is gone. */
  | 'handed_over'
  /** The app has it, but the browser copy could not be cancelled and stays paused in its list. */
  | 'copy_left'
  /** The app does not have it, and the browser resumed its copy. */
  | 'resumed'
  /** The app could not say whether it has it, and the browser resumed its copy too. */
  | 'uncertain'
  /** The browser would not resume its copy, which is still paused. */
  | 'stuck_paused';

export interface RecoveryDeps {
  downloads: DownloadControl;
  /** `withdraw_download`, connecting (or starting the app) first; rejects without an answer it can read. */
  withdraw(handoverId: string): Promise<WithdrawOutcome>;
}

/**
 * Settles one paused handover exactly as after a timeout: `withdraw_download` decides.
 * `already_started` means the app has it, so the browser copy goes; `withdrawn` or
 * `not_found` means it does not, so the browser resumes its copy; no answer, or one that
 * cannot be read, resumes the browser copy too, because two copies beat none.
 */
export async function recoverHandover(
  { id, handoverId }: InFlightEntry,
  deps: RecoveryDeps,
): Promise<{ result: Recovered; name: string }> {
  const { downloads } = deps;
  const state = await downloads.state(id).catch(() => null);
  const name = nonEmpty(baseName(state?.filename ?? '')) ?? 'A download';
  if (state?.state !== 'in_progress' || !state.paused) return { result: 'not_waiting', name };
  const outcome = await deps.withdraw(handoverId).catch(() => null);
  if (outcome === 'already_started') {
    const removed = await removeBrowserCopy(downloads, id);
    return { result: removed ? 'handed_over' : 'copy_left', name };
  }
  const copy = await resumeBrowserCopy(downloads, id);
  if (copy === 'stuck_paused') return { result: 'stuck_paused', name };
  if (copy === 'gone') return { result: 'not_waiting', name };
  return { result: outcome === null ? 'uncertain' : 'resumed', name };
}
