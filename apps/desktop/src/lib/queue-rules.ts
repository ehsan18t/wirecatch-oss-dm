// A queue's rules as its editor holds them (D72.2).

import type { Rule, QueueRules, AddDownloads } from '@wirecatch/protocol';
import { rulePhrase } from './queue-reasons';

/** A queue's rules for its form: an auto queue's own, else on and matching nothing yet. */
export function rulesOf(addDownloads: AddDownloads | undefined): QueueRules {
  if (addDownloads?.kind === 'auto') {
    const { enabled, sites, file_names, file_name_syntax, file_types, categories, protocols } =
      addDownloads;
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

/** What the form's rules make of the queue when it is saved. */
export function addDownloadsOf(auto: boolean, rules: QueueRules): AddDownloads {
  return auto ? { kind: 'auto', ...rules } : { kind: 'manual' };
}

/** An extension as a rule keeps it: no leading dot, lower case. */
export function fileTypeRule(typed: string): string | null {
  return typed.trim().replace(/^\.+/, '').toLowerCase() || null;
}

/** `list` with `item` in it (`on`) or out of it. */
export function toggled<T>(list: readonly T[], item: T, on: boolean): T[] {
  if (!on) return list.filter((x) => x !== item);
  return list.includes(item) ? [...list] : [...list, item];
}

/** An auto queue's rules in a few words, for a list of queues: "Adds youtube.com and 2 more". Null for a manual queue. */
export function rulesFact(addDownloads: AddDownloads): string | null {
  if (addDownloads.kind === 'manual') return null;
  if (!addDownloads.enabled) return 'Rules off';
  const all: Rule[] = [
    ...addDownloads.sites.map((value) => ({ kind: 'site' as const, value })),
    ...addDownloads.file_names.map((value) => ({ kind: 'file_name' as const, value })),
    ...addDownloads.file_types.map((value) => ({ kind: 'file_type' as const, value })),
    ...addDownloads.categories.map((value) => ({ kind: 'category' as const, value })),
    ...addDownloads.protocols.map((value) => ({ kind: 'protocol' as const, value })),
  ];
  const [first] = all;
  if (first === undefined) return 'No rules yet';
  const more = all.length - 1;
  return more === 0
    ? `Adds ${rulePhrase(first)}`
    : `Adds ${rulePhrase(first)} and ${String(more)} more`;
}

/** Whether `rules` name anything to rule by. */
export function hasRules(rules: QueueRules): boolean {
  const lists = [rules.sites, rules.file_types, rules.categories, rules.protocols];
  return lists.some((l) => l.length > 0) || rules.file_names.some((n) => n.trim() !== '');
}
