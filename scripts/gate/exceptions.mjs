// The lint exceptions: the only places a lint may be switched off, and the checks that hold the code to that list.
import { disableComments } from './comments.mjs';
import { GateError } from './process.mjs';
import { rustLintAttributes } from './rust-attributes.mjs';

/**
 * The only places a lint is switched off, each listed with its reason in docs/AUDIT.md W14. An entry is one attribute or comment, named by its file and exactly the lints it switches off, so it may move within its file; a new one, another lint, or a second one in the same file fails the `exceptions` check until it is reasoned about and listed here and in AUDIT.
 */
const LINT_EXCEPTIONS = [
  {
    id: 'E1',
    file: 'apps/desktop/src/lib/backend/mock/rng.ts',
    lints: ['wirecatch/no-magic-numbers'],
  },
  { id: 'E2', file: 'crates/protocol/src/wire.rs', lints: ['clippy::large_enum_variant'] },
  { id: 'E3', file: 'crates/engine/src/transport.rs', lints: ['clippy::large_enum_variant'] },
  { id: 'E4', file: 'crates/protocol/src/app.rs', lints: ['clippy::struct_excessive_bools'] },
  {
    id: 'E5',
    file: 'crates/e2e/src/lib.rs',
    lints: ['clippy::expect_used', 'clippy::missing_panics_doc'],
  },
  {
    id: 'E6',
    file: 'crates/test-support/src/lib.rs',
    lints: ['clippy::missing_panics_doc', 'clippy::unwrap_used'],
  },
  { id: 'E7', file: 'crates/engine/src/numeric.rs', lints: ['clippy::cast_precision_loss'] },
  {
    id: 'E8',
    file: 'crates/engine/src/numeric.rs',
    lints: ['clippy::cast_possible_truncation', 'clippy::cast_sign_loss'],
  },
  {
    id: 'E9',
    file: 'apps/desktop/src/ui/download/FileQuestion.svelte',
    lints: ['a11y_no_noninteractive_element_interactions'],
  },
  {
    id: 'E10',
    file: 'apps/desktop/src/ui/download/MediaQuestion.svelte',
    lints: ['a11y_no_noninteractive_element_interactions'],
  },
  {
    id: 'E11',
    file: 'apps/desktop/src/ui/download/ProgressView.svelte',
    lints: ['a11y_no_static_element_interactions'],
  },
  { id: 'E12', file: 'crates/core/src/addons/manifest.rs', lints: ['clippy::expect_used'] },
  {
    id: 'E13',
    file: 'apps/desktop/src/ui/download/WindowFrame.svelte',
    lints: ['a11y_no_noninteractive_tabindex'],
  },
];
const WHERE_LISTED = 'LINT_EXCEPTIONS in scripts/gate/exceptions.mjs and docs/AUDIT.md W14';

/** Every `#[expect]`, `#[allow]`, ESLint disable comment and svelte-ignore comment is one of LINT_EXCEPTIONS, and every listed exception is still in the code. */
function lintExceptions() {
  const unused = [...LINT_EXCEPTIONS];
  const unlisted = [];
  for (const site of [...rustLintAttributes(), ...disableComments()]) {
    const lints = site.lints.join(', ');
    const index = unused.findIndex(
      (e) => e.file === site.file && [...e.lints].sort().join(', ') === lints,
    );
    if (index === -1) unlisted.push(`${site.file}:${String(site.line)}: ${site.kind} ${lints}`);
    else unused.splice(index, 1);
  }
  const messages = [];
  if (unlisted.length > 0) {
    messages.push(
      `Lints switched off outside the listed exceptions. Fix the cause; only where the lint is wrong for that code, keep one exception with its reason and list it in ${WHERE_LISTED}:\n${unlisted.join('\n')}`,
    );
  }
  if (unused.length > 0) {
    const list = unused.map((e) => `${e.id}: ${e.file}: ${e.lints.join(', ')}`).join('\n');
    messages.push(
      `Listed exceptions no longer in the code; remove them from ${WHERE_LISTED}:\n${list}`,
    );
  }
  if (messages.length > 0) throw new GateError(messages.join('\n\n'));
}

/**
 * `#[allow]` is already an error through clippy's `allow_attributes`, but clippy only sees the attributes that are switched on for the system it runs on: `#[cfg_attr(not(windows), allow(...))]` passes on Windows and fails on Linux. This finds them on every system.
 */
function conditionalAllows() {
  const hits = rustLintAttributes().filter((a) => a.conditional && a.kind === 'allow');
  if (hits.length > 0) {
    const list = hits
      .map((a) => `${a.file}:${String(a.line)}: allow(${a.lints.join(', ')})`)
      .join('\n');
    throw new GateError(
      `\`allow\` inside \`cfg_attr\` is not used: clippy cannot check it on the systems where it is switched off. Fix the cause (for code unused on one system, put it behind the same \`cfg\` as its users), or write \`#[expect(..., reason = "...")]\` and list it as an exception:\n${list}`,
    );
  }
}

export const EXCEPTION_CHECKS = {
  'cfg-allow': conditionalAllows,
  exceptions: lintExceptions,
};
