// The samples the redaction tests read, from `crates/test-support/data/redaction.json`: the one file the log's tests (`wirecatch_test_support::redaction`) and the extension's both load, so the two redactions are held to the same cases (D40.10). For tests only; the package exports it as `@wirecatch/protocol/testing/redaction`.
import data from '../../../../crates/test-support/data/redaction.json';

/** One of the ways text is redacted: the log's `text_for_log`, the log's `text_for_display`, the extension's `redact`. */
export type Side = 'log' | 'display' | 'diagnostics';

/** A text written whole, or as pieces so that no file holds a string a secret scanner takes for a real credential. */
type Pieces = string | readonly string[];

/** One sample and what each side makes of it, its pieces joined. */
export interface RedactionCase {
  readonly text: string;
  /** What neither the log nor the diagnostics may keep, in any of the wrappings, unless one skips the case. */
  readonly secrets: readonly string[];
  /** What the diagnostics may not keep, which the log keeps on purpose. */
  readonly private: readonly string[];
  readonly log: string | null;
  readonly display: string | null;
  readonly diagnostics: string | null;
  /** The sides that leave the text as it is. */
  readonly keptBy: readonly Side[];
  /** The redaction that does not read this form on purpose. */
  readonly skippedBy: Side | null;
}

/** One input of a rule, and its answer. */
export interface RuleCase {
  readonly input: string;
  readonly answer: boolean;
}

const SIDES: readonly string[] = ['log', 'display', 'diagnostics'];
const PLACE = '{}';

/** `pieces` joined into the text they stand for. */
export const joined = (pieces: Pieces): string =>
  typeof pieces === 'string' ? pieces : pieces.join('');

function side(value: string): Side {
  if (!SIDES.includes(value))
    throw new Error(`redaction.json names no side ${JSON.stringify(value)}`);
  return value as Side;
}

/** Every sample. */
export function redactionCases(): RedactionCase[] {
  return data.cases.map((written) => {
    const read: Partial<Record<string, unknown>> = written;
    const text = (name: string): string | null =>
      typeof read[name] === 'string' ? read[name] : null;
    const list = (name: string): Pieces[] =>
      Array.isArray(read[name]) ? (read[name] as Pieces[]) : [];
    return {
      text: joined(written.text),
      secrets: list('secrets').map(joined),
      private: list('private').map(joined),
      log: text('log'),
      display: text('display'),
      diagnostics: text('diagnostics'),
      keptBy: list('kept_by').map((name) => side(joined(name))),
      skippedBy: text('skipped_by') === null ? null : side(text('skipped_by') ?? ''),
    };
  });
}

/** What `on` must make of the case's text, if the case says. */
export function expected(sample: RedactionCase, on: Side): string | null {
  const exact = on === 'log' ? sample.log : on === 'display' ? sample.display : sample.diagnostics;
  return exact ?? (sample.keptBy.includes(on) ? sample.text : null);
}

/** The secrets `on` must hide: none when it skips the case, or for `display`. */
export const secretsFor = (sample: RedactionCase, on: Side): readonly string[] =>
  on === 'display' || sample.skippedBy === on ? [] : sample.secrets;

/** The cases of the rule `name`, its inputs joined. */
export function ruleCases(name: string): RuleCase[] {
  const rules: Partial<Record<string, readonly { input: Pieces; answer: boolean }[]>> = data.rules;
  const cases = rules[name];
  if (cases === undefined) throw new Error(`redaction.json holds no cases of ${name}`);
  return cases.map(({ input, answer }) => ({ input: joined(input), answer }));
}

/** `text` in each of the surroundings a secret is tested in. */
export const wrappings = (text: string): string[] =>
  data.wrappings.map((wrapping) => wrapping.replace(PLACE, () => text));
