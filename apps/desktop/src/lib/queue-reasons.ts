// Why a download is in its queue, in one line every window shares: "Queue: Videos. Its rules match youtube.com" (D72.5, D72.8).

import type { Rule, LinkMatch, LinkProtocol, QueueId, QueueReason } from '@wirecatch/protocol';

const PROTOCOL_LABEL: Record<LinkProtocol, string> = {
  http: 'HTTP',
  https: 'HTTPS',
  ftp: 'FTP',
};

const CATEGORY_TAKEN = {
  video: 'videos',
  audio: 'music and audio',
  archive: 'archives',
  document: 'documents',
  program: 'programs',
  image: 'images',
  other: 'other files',
} as const;

/** What `rule` matches: "youtube.com", "files named *.iso". */
export function rulePhrase(rule: Rule): string {
  switch (rule.kind) {
    case 'site':
      return rule.value;
    case 'file_name':
      return `files named ${rule.value}`;
    case 'file_type':
      return `.${rule.value} files`;
    case 'category':
      return CATEGORY_TAKEN[rule.value];
    case 'protocol':
      return `${PROTOCOL_LABEL[rule.value]} links`;
  }
}

/** Why a download is in its queue, after its name. */
export function reasonPhrase(reason: QueueReason): string {
  switch (reason.kind) {
    case 'picked':
      return 'You picked it.';
    case 'matched':
      return `Its rules match ${rulePhrase(reason.rule)}.`;
    case 'browser_queue':
      return 'It is the queue for browser downloads.';
    case 'default_queue':
      return 'No queue’s rules match it.';
  }
}

/** "Queue: Videos. Its rules match youtube.com." */
export function queueLine(name: string, reason: QueueReason): string {
  return `Queue: ${name}. ${reasonPhrase(reason)}`;
}

/** Where the rules send the Add dialog's links: why for one link, how many go to each queue for several (D72.8). */
export function placementLine(placed: readonly { name: string; reason: QueueReason }[]): string {
  const [first] = placed;
  if (first === undefined) return '';
  if (placed.length === 1) return queueLine(first.name, first.reason);
  const counts = new Map<string, number>();
  for (const { name } of placed) counts.set(name, (counts.get(name) ?? 0) + 1);
  if (counts.size === 1) return `Queue: ${first.name}, for all ${String(placed.length)} links.`;
  const parts = [...counts].map(([name, n], i) =>
    i === 0 ? `${String(n)} link${n === 1 ? '' : 's'} to ${name}` : `${String(n)} to ${name}`,
  );
  return `Queue: ${parts.join(', ')}.`;
}

/** What Test a link says: where the link goes and why, then the switched-off queue that would have matched it. `name` names a queue, `null` being the one not made yet; `fallback` is where a link nothing rules goes. */
export function trialLines(
  found: LinkMatch,
  name: (queue: QueueId | null) => string,
  fallback: string,
): string[] {
  const lines = [
    found.matched
      ? queueLine(name(found.matched.queue), { kind: 'matched', rule: found.matched.rule })
      : queueLine(fallback, { kind: 'default_queue' }),
  ];
  if (found.switched_off) {
    const { queue, rule } = found.switched_off;
    lines.push(`${name(queue)} would match ${rulePhrase(rule)}, but its rules are off.`);
  }
  return lines;
}
