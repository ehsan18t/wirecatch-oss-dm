// A queue's claims as its editor holds them (D72.2).

import type { Claim, QueueClaims, QueueTakes } from '@wirecatch/protocol';
import { claimPhrase } from './queue-reasons';

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

/** An auto queue's claims in a few words, for a list of queues: "Takes youtube.com and 2 more". Null for a manual queue. */
export function claimsFact(takes: QueueTakes): string | null {
  if (takes.kind === 'manual') return null;
  if (!takes.enabled) return 'Auto, claims off';
  const all: Claim[] = [
    ...takes.sites.map((claim) => ({ kind: 'site' as const, claim })),
    ...takes.file_names.map((claim) => ({ kind: 'file_name' as const, claim })),
    ...takes.file_types.map((claim) => ({ kind: 'file_type' as const, claim })),
    ...takes.categories.map((claim) => ({ kind: 'category' as const, claim })),
    ...takes.protocols.map((claim) => ({ kind: 'protocol' as const, claim })),
  ];
  const [first] = all;
  if (first === undefined) return 'Auto, claims nothing yet';
  const more = all.length - 1;
  return more === 0
    ? `Takes ${claimPhrase(first)}`
    : `Takes ${claimPhrase(first)} and ${String(more)} more`;
}

/** Whether `claims` name anything to claim by. */
export function hasClaims(claims: QueueClaims): boolean {
  const lists = [claims.sites, claims.file_types, claims.categories, claims.protocols];
  return lists.some((l) => l.length > 0) || claims.file_names.some((n) => n.trim() !== '');
}
