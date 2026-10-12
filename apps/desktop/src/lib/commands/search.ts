// How the palette finds commands from what is typed, and groups them when nothing is.

import type { Command, CommandGroup } from './table';

// How the palette ranks a match. Each typed word scores against its best word in the text.
/** The typed word is the whole word. */
const SCORE_WHOLE_WORD = 12;
/**
 * The typed word starts the word. Up to one more for how much of the word is typed, so "pau"
 * ranks "Pause" above "Paused" but never above a whole word.
 */
const SCORE_PREFIX = 8;
/** The typed word is a short form of the word ("pse" for "Pause"). */
const SCORE_SHORT_FORM = 3;
/** Added when the words typed appear together and in order in the text. */
const SCORE_PHRASE = 5;
/** A match in a command's keywords counts for this share of one in its title. */
const KEYWORD_WEIGHT = 0.5;

function words(text: string): string[] {
  return text
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter(Boolean);
}

function isSubsequence(needle: string, hay: string): boolean {
  let i = 0;
  for (const ch of hay) if (ch === needle[i]) i += 1;
  return i === needle.length;
}

/**
 * Scores how well `query` matches `text`. Every typed word must match the start of a word in
 * the text ("pau" finds "Pause"), or be a short form of one word that starts with the same
 * letter ("pse" finds "Pause"). Letters scattered across words do not count, so "move" does
 * not find "Resume". `-1` means no match.
 */
export function fuzzyScore(query: string, text: string): number {
  const typed = words(query);
  if (typed.length === 0) return 0;
  const available = words(text);
  let score = 0;
  for (const q of typed) {
    let best = -1;
    for (const w of available) {
      if (w === q) best = Math.max(best, SCORE_WHOLE_WORD);
      else if (w.startsWith(q)) best = Math.max(best, SCORE_PREFIX + q.length / w.length);
      else if (q.length >= 2 && w.startsWith(q.charAt(0)) && isSubsequence(q, w))
        best = Math.max(best, SCORE_SHORT_FORM);
    }
    if (best < 0) return -1;
    score += best;
  }
  // The words typed together, in order, beat the same words spread out.
  if (text.toLowerCase().includes(query.toLowerCase().trim())) score += SCORE_PHRASE;
  return score;
}

export function searchCommands(commands: readonly Command[], query: string): Command[] {
  if (!query.trim()) return [...commands];
  const scored: { c: Command; s: number }[] = [];
  for (const c of commands) {
    const titleScore = fuzzyScore(query, c.title);
    // Keywords count for less than the title, but let "bandwidth" find the speed limit.
    const kwScore = c.keywords
      ? fuzzyScore(query, `${c.title} ${c.keywords}`) * KEYWORD_WEIGHT
      : -1;
    const s = Math.max(titleScore, kwScore);
    if (s >= 0) scored.push({ c, s });
  }
  scored.sort((a, b) => b.s - a.s);
  return scored.map((x) => x.c);
}

/** Commands in groups, in the order each group first appears; each keeps its own order inside. */
export function groupCommands(
  commands: readonly Command[],
): { name: CommandGroup; items: Command[] }[] {
  const groups = new Map<CommandGroup, Command[]>();
  for (const c of commands) {
    const items = groups.get(c.group);
    if (items) items.push(c);
    else groups.set(c.group, [c]);
  }
  return [...groups].map(([name, items]) => ({ name, items }));
}
