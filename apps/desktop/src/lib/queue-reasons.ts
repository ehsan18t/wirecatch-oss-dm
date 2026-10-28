// Why a download is in its queue, in one line every window shares: "Queue: Videos. It takes youtube.com" (D72.5, D72.8).

import type { Claim, LinkClaims, LinkProtocol, QueueId, QueueReason } from '@wirecatch/protocol';

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

/** What a queue takes by `claim`: "youtube.com", "files named *.iso". */
export function claimPhrase(claim: Claim): string {
  switch (claim.kind) {
    case 'site':
      return claim.claim;
    case 'file_name':
      return `files named ${claim.claim}`;
    case 'file_type':
      return `.${claim.claim} files`;
    case 'category':
      return CATEGORY_TAKEN[claim.claim];
    case 'protocol':
      return `${PROTOCOL_LABEL[claim.claim]} links`;
  }
}

/** Why a download is in its queue, after its name. */
export function reasonPhrase(reason: QueueReason): string {
  switch (reason.kind) {
    case 'picked':
      return 'You picked it.';
    case 'claimed':
      return `It takes ${claimPhrase(reason.claim)}.`;
    case 'browser_queue':
      return "It is the browser's queue.";
    case 'default_queue':
      return 'No queue claims it.';
  }
}

/** "Queue: Videos. It takes youtube.com." */
export function queueLine(name: string, reason: QueueReason): string {
  return `Queue: ${name}. ${reasonPhrase(reason)}`;
}

/** Where the claims send the Add dialog's links: why for one link, how many go to each queue for several (D72.8). */
export function placementLine(placed: readonly { name: string; reason: QueueReason }[]): string {
  const [first] = placed;
  if (first === undefined) return 'Each link goes to the queue that takes it.';
  if (placed.length === 1) return queueLine(first.name, first.reason);
  const counts = new Map<string, number>();
  for (const { name } of placed) counts.set(name, (counts.get(name) ?? 0) + 1);
  if (counts.size === 1) return `Queue: ${first.name}, for all ${String(placed.length)} links.`;
  const parts = [...counts].map(([name, n], i) =>
    i === 0 ? `${String(n)} link${n === 1 ? '' : 's'} to ${name}` : `${String(n)} to ${name}`,
  );
  return `Queue: ${parts.join(', ')}.`;
}

/** What Try a link says: where the link goes and why, then the switched-off queue that would have taken it. `name` names a queue, `null` being the one not made yet; `fallback` is where a link nothing claims goes. */
export function trialLines(
  found: LinkClaims,
  name: (queue: QueueId | null) => string,
  fallback: string,
): string[] {
  const lines = [
    found.taken
      ? queueLine(name(found.taken.queue), { kind: 'claimed', claim: found.taken.claim })
      : queueLine(fallback, { kind: 'default_queue' }),
  ];
  if (found.switched_off) {
    const { queue, claim } = found.switched_off;
    lines.push(`${name(queue)} would take ${claimPhrase(claim)}, but its claims are off.`);
  }
  return lines;
}
