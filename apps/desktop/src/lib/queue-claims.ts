// A queue's claims as its editor holds them (D72.2).

import type { QueueClaims, QueueTakes } from '@wirecatch/protocol';

/** A queue's claims for its form: an auto queue's own, else on and claiming nothing yet. */
export function claimsOf(takes: QueueTakes | undefined): QueueClaims {
  if (takes?.kind === 'auto') {
    const { enabled, sites, file_names, file_name_syntax, file_types, categories, protocols } =
      takes;
    return { enabled, sites, file_names, file_name_syntax, file_types, categories, protocols };
  }
  return {
    enabled: true,
    sites: [],
    file_names: [],
    file_name_syntax: 'wildcards',
    file_types: [],
    categories: [],
    protocols: [],
  };
}

/** What the form's claims make of the queue when it is saved. */
export function takesOf(auto: boolean, claims: QueueClaims): QueueTakes {
  return auto ? { kind: 'auto', ...claims } : { kind: 'manual' };
}

/** An extension as a claim keeps it: no leading dot, lower case. */
export function fileTypeClaim(typed: string): string | null {
  return typed.trim().replace(/^\.+/, '').toLowerCase() || null;
}

/** `list` with `item` in it (`on`) or out of it. */
export function toggled<T>(list: readonly T[], item: T, on: boolean): T[] {
  if (!on) return list.filter((x) => x !== item);
  return list.includes(item) ? [...list] : [...list, item];
}
