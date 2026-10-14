// What the mock core does when the person asks for a newer Public Suffix List (D44).

import type { SuffixListInfo } from '@wirecatch/protocol';
import { BackendError } from '../types';
import type { MockState } from './state';

/** What the next check finds; tests set it. */
export type SuffixCheck = 'up-to-date' | 'newer' | 'unreachable';

/** When the list a check that finds a newer one takes was published. */
const NEWER_PUBLISHED = Date.parse('2026-10-01T12:00:00Z');

/**
 * Looks for a newer list as the core does: every window hears how it ended, and a failure keeps
 * the list in use and says so.
 */
export function checkSuffixList(state: MockState, finds: SuffixCheck): SuffixListInfo {
  const list = state.suffixList;
  list.checked = state.now();
  list.check_failed = finds === 'unreachable';
  if (finds === 'newer') list.published = NEWER_PUBLISHED;
  state.emit({ type: 'suffix_list_checked', data: { ...list } });
  if (list.check_failed) {
    throw new BackendError(
      'Wirecatch could not update its list of public domains (no connection). It keeps using the list it has; check your internet connection, then try again.',
      'update_check_failed',
    );
  }
  return { ...list };
}
