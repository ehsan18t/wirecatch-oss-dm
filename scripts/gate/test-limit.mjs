// Runs `cargo test` so that a test that hangs fails the check by name, before the CI job's own limit cancels the job and loses its log (D174).
import { spawn, spawnSync } from 'node:child_process';
import { describe, GateError, ROOT, startFailure } from './process.mjs';

/** Added to libtest's notice at 60 seconds, so a test gets 5 minutes: two `PATIENCE` waits that run out, and then some. */
export const LIMIT_AFTER_NOTICE_MS = 4 * 60 * 1000;
/** How long to read on after cargo exits: a child a test leaked can hold the pipe open forever. */
const DRAIN_MS = 5000;

// Not anchored: a child a test started shares this pipe and can leave half a line before them.
const NOTICE = /\btest (.+) has been running for over \d+ seconds/;
const RESULT = /\btest (.+) \.\.\. /;
/** What libtest adds to a name in a result line, and not in its notice. */
const TEST_MODE = / - (?:should panic|compile fail|compile)$/;
/** A binary's first and last lines: one that aborts prints no last line. */
const BINARY_EDGE = /\btest result: |\brunning \d+ tests?\b/;

/** The signals that end this process: the test binaries, in their own group, would run on. */
const ENDING_SIGNALS = ['SIGINT', 'SIGTERM', 'SIGHUP'];

/** Follows libtest's lines and calls `onHang` when a test it gave notice of runs out its limit. */
function testWatch(limitMs, onHang) {
  const timers = new Map();
  const forget = (name) => {
    clearTimeout(timers.get(name));
    timers.delete(name);
  };
  const clear = () => {
    for (const name of timers.keys()) forget(name);
  };
  const line = (text) => {
    const notice = NOTICE.exec(text);
    const result = RESULT.exec(text);
    if (notice) {
      timers.set(
        notice[1],
        setTimeout(() => {
          onHang([...timers.keys()]);
        }, limitMs),
      );
    } else if (result) forget(result[1].replace(TEST_MODE, ''));
    else if (BINARY_EDGE.test(text)) clear();
  };
  return { line, clear };
}

/** Stops cargo and the test binary under it. */
function stopTree(child) {
  if (process.platform === 'win32') {
    spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
  } else {
    try {
      process.kill(-child.pid, 'SIGKILL');
    } catch (error) {
      // The group is gone once its last process has ended.
      if (error.code !== 'ESRCH') throw error;
    }
  }
}

/** Own process group on Unix, so the whole tree can be stopped: Ctrl+C and a closed terminal then no longer reach it by themselves. */
function passOnInterrupt(child) {
  if (process.platform === 'win32') return () => undefined;
  const interrupted = () => {
    stopTree(child);
    process.exit(1);
  };
  for (const signal of ENDING_SIGNALS) process.on(signal, interrupted);
  return () => {
    for (const signal of ENDING_SIGNALS) process.off(signal, interrupted);
  };
}

function hangError(names, limitMs) {
  const minutes = Math.round(limitMs / 60_000) + 1;
  return new GateError(
    `Stopped the run, so later test binaries did not run: no result after about ${String(minutes)} minutes from ${names.join(', ')}. Find what the test waits for; a wait is bounded by PATIENCE.`,
  );
}

/** Shows the command's output as `run` does, and throws a `GateError` naming any test that hangs. */
export function runWithTestLimit(
  command,
  args,
  { env, limitMs = LIMIT_AFTER_NOTICE_MS, drainMs = DRAIN_MS, output = process.stdout } = {},
) {
  console.log(`\n$ ${[command, ...args].join(' ')}`);
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: ROOT,
      stdio: ['inherit', 'pipe', 'inherit'],
      env: { ...process.env, ...env },
      detached: process.platform !== 'win32',
    });
    let hung = null;
    let rest = '';
    const watch = testWatch(limitMs, (names) => {
      hung ??= names;
      stopTree(child);
    });
    const stopForwarding = passOnInterrupt(child);
    child.on('error', (error) => {
      stopForwarding();
      reject(startFailure(command, args, error));
    });
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (chunk) => {
      output.write(chunk);
      const lines = (rest + chunk).split('\n');
      rest = lines.pop();
      lines.forEach(watch.line);
    });
    child.on('exit', () => {
      setTimeout(() => {
        child.stdout.destroy();
      }, drainMs).unref();
    });
    child.on('close', (status) => {
      stopForwarding();
      watch.clear();
      if (hung) reject(hangError(hung, limitMs));
      else if (status === 0) {
        resolve();
      } else {
        reject(new GateError(`${describe(command, args)} failed with status ${String(status)}.`));
      }
    });
  });
}
