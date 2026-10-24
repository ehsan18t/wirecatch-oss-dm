// Rules about the text of every file: no em dashes, no string a secret scanner takes for a provider's credential, and a reason on every comment that switches a warning off where no linter reads it.
import { disableComments } from './comments.mjs';
import { GateError, grepRepository } from './process.mjs';

/** The em dash's code point. */
const EM_DASH = 0x2014;
/** Ways an em dash gets into the repository. Escapes only count in TypeScript, JavaScript and Svelte outside tests, where interface text lives: tests use them to assert there is none, and Rust tests do the same inside `#[cfg(test)]` modules that a path cannot tell apart. */
// The character is built and the entity split so this line does not match itself. The character itself, not `\x{2014}`, goes to git: without a UTF-8 locale git's PCRE refuses code points above 255, and a literal matches its bytes in any locale.
export const EM_DASH_LITERAL = `${String.fromCodePoint(EM_DASH)}|&${'mdash;'}`;
const EM_DASH_ESCAPED = String.raw`\\u2014|\\u\{2014\}`;
const TEXT_SCOPE = ['.', ':!Cargo.lock', ':!pnpm-lock.yaml'];
const ESCAPE_SCOPE = [
  '*.ts',
  '*.js',
  '*.mjs',
  '*.svelte',
  ':!scripts/gate.mjs',
  ':!scripts/gate/**',
  ':!**/tests/**',
  ':!**/*test*',
  ':!**/testing/**',
];

function emDashes() {
  const hits = [
    ...grepRepository(EM_DASH_LITERAL, TEXT_SCOPE),
    ...grepRepository(EM_DASH_ESCAPED, ESCAPE_SCOPE),
  ];
  if (hits.length > 0) {
    throw new GateError(
      `Em dashes are not used in this repository; use a comma, colon, parentheses or a new sentence:\n${hits.join('\n')}`,
    );
  }
}

/**
 * Credentials as providers shape them, which secret scanners (GitHub's push protection among them) refuse in a push: Stripe, Anthropic and OpenAI keys, GitHub, GitLab and npm tokens, Slack tokens, AWS access key ids, Google API keys, JWTs and private key blocks. A sample a test needs is stored in pieces in a data file (`crates/test-support/data/redaction.json`) and joined by its loader. The thresholds sit below the providers' own lengths, so a sample a little short of the real shape is caught too. No pattern matches its own source line.
 */
export const CREDENTIAL_PATTERN = [
  String.raw`\b[prs]k_(?:live|test)_[0-9A-Za-z]{16,}`,
  String.raw`\bsk-(?:live|test|ant|proj)-[0-9A-Za-z_-]{16,}`,
  String.raw`\bgh[oprsu]_[0-9A-Za-z]{16,}`,
  String.raw`\bgithub_pat_[0-9A-Za-z_]{16,}`,
  String.raw`\bglpat-[0-9A-Za-z_-]{16,}`,
  String.raw`\bnpm_[0-9A-Za-z]{16,}`,
  String.raw`\bxox[abprs]-[0-9A-Za-z-]{10,}`,
  String.raw`\b(?:AKIA|ASIA)[0-9A-Z]{16}\b`,
  String.raw`\bAIza[0-9A-Za-z_-]{20,}`,
  String.raw`\beyJ[0-9A-Za-z_-]{8,}\.eyJ[0-9A-Za-z_-]{4,}\.[0-9A-Za-z_-]{6,}`,
  '-----BEGIN (?:[A-Z]+ )*PRIVATE KEY-----',
].join('|');
const CREDENTIAL_SCOPE = ['.', ':!Cargo.lock', ':!pnpm-lock.yaml'];

function credentialShapes() {
  const hits = grepRepository(CREDENTIAL_PATTERN, CREDENTIAL_SCOPE);
  if (hits.length > 0) {
    throw new GateError(
      `A string shaped like a provider's credential is refused by secret scanners, and a real one must never be committed. Store a test sample in pieces (["AKIA", "..."]) in a data file and join it in its loader:\n${hits.join('\n')}`,
    );
  }
}

/** How an HTML comment opens, split so these examples are not comments themselves. */
const MARKUP = ['<', '!--'].join('');
/** The ESLint comments the shared configuration allows in scripts (`no-use`); Svelte markup, which that rule does not read, is held to the same list here. */
const ALLOWED_IN_MARKUP = new Set([
  'eslint-disable',
  'eslint-disable-next-line',
  'eslint-enable',
  'svelte-ignore',
]);

/**
 * Comments that switch a warning off where no linter holds them to a reason: every svelte-ignore (Svelte does not lint its own comments), and ESLint comments in Svelte markup, which eslint-plugin-eslint-comments does not read. ESLint comments in scripts are held to a reason by the shared configuration (`require-description`).
 */
function unreasonedComments() {
  const unreasoned = disableComments().filter(
    (c) => (c.kind === 'svelte-ignore' || c.markup) && !c.reason,
  );
  if (unreasoned.length > 0) {
    const list = unreasoned
      .map((c) => `${c.file}:${String(c.line)}: ${c.kind} ${c.lints.join(', ')}`)
      .join('\n');
    throw new GateError(
      `A comment that switches a warning off says why it is wrong there: \`${MARKUP} svelte-ignore a11y_x (the reason) -->\`, \`${MARKUP} eslint-disable-next-line rule -- the reason -->\`. Fix the cause where you can:\n${list}`,
    );
  }
}

function unsupportedMarkupComments() {
  const found = disableComments().filter((c) => c.markup && !ALLOWED_IN_MARKUP.has(c.kind));
  if (found.length > 0) {
    const list = found.map((c) => `${c.file}:${String(c.line)}: ${c.kind}`).join('\n');
    throw new GateError(
      `Svelte markup takes the same ESLint comments as scripts: eslint-disable-next-line (not eslint-disable-line), or eslint-disable with eslint-enable, never rule settings, globals or environments:\n${list}`,
    );
  }
}

/** Runs every text rule and reports all that fail, not only the first. */
function textRules() {
  const failures = [];
  for (const rule of [emDashes, credentialShapes, unreasonedComments, unsupportedMarkupComments]) {
    try {
      rule();
    } catch (error) {
      if (!(error instanceof GateError)) throw error;
      failures.push(error.message);
    }
  }
  if (failures.length > 0) throw new GateError(failures.join('\n\n'));
}

export const TEXT_CHECKS = { text: textRules };
