// Running commands for the gate, and what every check shares: the repository root, the options, and the error a failed check throws.
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

/** Upper bound on output read back from a command (cargo metadata of the whole workspace is a few MiB). */
const MAX_CAPTURED_BYTES = 64 * 1024 * 1024;

/** A check failed; its message says what is wrong and how to fix it. */
export class GateError extends Error {}

/** Command-line options. */
export const options = { desktop: true, failFast: true };

/**
 * How to start a command. pnpm is a `.cmd` script on Windows and only starts through a shell, so it is passed as one command line built from the gate's own constant arguments. cargo, git and rustup are real executables and start without a shell, so `%` and quotes in their arguments reach them unchanged.
 */
function invocation(command, args) {
  if (process.platform === 'win32' && command === 'pnpm') {
    return { file: [command, ...args].join(' '), args: [], shell: true };
  }
  return { file: command, args, shell: false };
}

function describe(command, args) {
  return `\`${[command, ...args].join(' ')}\``;
}

function startFailure(command, args, error) {
  return new GateError(
    `Could not start ${describe(command, args)}: ${error.message}. Check that ${command} is installed and on your PATH.`,
  );
}

/** Runs a command with its output shown, and fails the check when it fails. */
export function run(command, args, { env } = {}) {
  console.log(`\n$ ${[command, ...args].join(' ')}`);
  const how = invocation(command, args);
  const result = spawnSync(how.file, how.args, {
    cwd: ROOT,
    stdio: 'inherit',
    shell: how.shell,
    env: { ...process.env, ...env },
  });
  if (result.error) throw startFailure(command, args, result.error);
  if (result.status !== 0) {
    throw new GateError(`${describe(command, args)} failed with status ${String(result.status)}.`);
  }
}

/** Runs a command and returns what it printed. `allowed` lists exit statuses that are answers rather than failures (git grep exits 1 when nothing matches). */
export function capture(command, args, { allowed = [0], hint = '' } = {}) {
  const how = invocation(command, args);
  const result = spawnSync(how.file, how.args, {
    cwd: ROOT,
    encoding: 'utf8',
    shell: how.shell,
    maxBuffer: MAX_CAPTURED_BYTES,
  });
  if (result.error) throw startFailure(command, args, result.error);
  if (!allowed.includes(result.status)) {
    const detail = result.stderr.trim();
    throw new GateError(
      `${describe(command, args)} failed with status ${String(result.status)}${detail ? `:\n${detail}` : '.'}${hint ? `\n${hint}` : ''}`,
    );
  }
  return { out: result.stdout, err: result.stderr, status: result.status };
}

export const cargo = (...args) => run('cargo', args);
export const pnpm = (...args) => run('pnpm', args);

export function cargoMetadata({ deps = false } = {}) {
  const args = ['metadata', '--format-version', '1', '--locked'];
  if (!deps) args.push('--no-deps');
  return JSON.parse(capture('cargo', args).out);
}

/** Lines matching `pattern` in tracked and untracked (not ignored) text files; git decides what is binary. */
export function grepRepository(pattern, pathspecs) {
  const { out } = capture(
    'git',
    ['grep', '--untracked', '-I', '-n', '-P', pattern, '--', ...pathspecs],
    { allowed: [0, 1] },
  );
  return out.split('\n').filter(Boolean);
}

/** The files, tracked and untracked (not ignored), that contain a match for `pattern`. */
export function filesMatching(pattern, pathspecs) {
  const { out } = capture(
    'git',
    ['grep', '-l', '--untracked', '-I', '-P', pattern, '--', ...pathspecs],
    { allowed: [0, 1] },
  );
  return out.split('\n').filter(Boolean);
}
