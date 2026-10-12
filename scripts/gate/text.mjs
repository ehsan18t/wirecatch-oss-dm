// Rules about the text of every file: no em dashes, and a reason on every comment that switches a warning off where no linter reads it.
import { disableComments } from './comments.mjs';
import { GateError, grepRepository } from './process.mjs';

/** Ways an em dash gets into the repository. Escapes only count in TypeScript, JavaScript and Svelte outside tests, where interface text lives: tests use them to assert there is none, and Rust tests do the same inside `#[cfg(test)]` modules that a path cannot tell apart. */
// The entity is split so this line does not match itself.
const EM_DASH_LITERAL = '\\x{2014}|&' + 'mdash;';
const EM_DASH_ESCAPED = '\\\\u2014|\\\\u\\{2014\\}';
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

/** ESLint's own comments in scripts are held to a reason by the shared config (eslint-comments/require-description); Svelte's are not linted, so they are checked here. */
function unreasonedSvelteIgnores() {
  const unreasoned = disableComments().filter((c) => c.kind === 'svelte-ignore' && !c.reason);
  if (unreasoned.length > 0) {
    const list = unreasoned
      .map((c) => `${c.file}:${String(c.line)}: svelte-ignore ${c.lints.join(', ')}`)
      .join('\n');
    throw new GateError(
      `A svelte-ignore comment says why the warning is wrong there, after \`--\` (\`<!-- svelte-ignore a11y_x -- the reason -->\`). Fix the cause where you can:\n${list}`,
    );
  }
}

export const TEXT_CHECKS = {
  text: () => {
    emDashes();
    unreasonedSvelteIgnores();
  },
};
