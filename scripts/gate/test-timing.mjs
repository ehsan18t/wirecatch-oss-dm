// Refuses Rust tests that race the machine: an upper bound on how long something took, or a short fixed timeout on a wait that has to succeed. Such a test passes on a fast, idle machine and fails on a busy CI runner, whatever the change being tested (ENGINEERING section 7).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, GateError, ROOT } from './process.mjs';
import { closingBracket, codeOnly } from './rust-attributes.mjs';

/** What measures how long something took: an `Instant`'s `elapsed()` or `duration_since`, or the gap between two instants a test server recorded (`Seen::at`). */
const MEASURED = String.raw`\.elapsed\(\)|duration_since\(|\.at\s*-`;

/** The waits the check reads. `\btimeout` keeps out configured limits such as `with_capture_timeout`, so `recv_timeout` is named. */
const WAIT = String.raw`(?:\btimeout|\brecv_timeout|_within)`;

/** A wait given a fixed `Duration` written in place: `timeout(Duration::from_secs(2), ..)`, `outcome_within(id, Duration::from_secs(1))`. */
const FIXED_WAIT = String.raw`${WAIT}\((?:[^()]|\([^()]*\))*?Duration::from_\w+\(`;

/** A server told to send slowly, also through a helper: the test relies on the speed to act before it finishes. */
const THROTTLED =
  /\b(?:throttled|slow)_server\(|\bstrict_server\([^;]*\bSome\(|\.throttle\([^;]*\bSome\(|\bper_connection_bps\s*[:=]\s*Some\b|\bdata_bps\s*[:=]\s*Some\b|\bserver_naming\([^;]*\bSome\(/g;

/** What a test does to a download while it runs. */
const ACTS_ON_A_RUN =
  /\.(?:pause|pause_all|cancel|resume|resume_all|retry|refresh_link|shutdown)\(|\.remove\(&|\.set_download_folder\(|\.restart\(|\.update_proxy_server\(|\.clock\.set\(|\.cancel_addon_operation\(|\.remove_addon\(|\btune\(|\bcancel_when\(|\bpause_midway\(|\banother_program_saves_meanwhile\(/g;

/** Below this, a named wait or deadline is short: `PATIENCE` in test-support, two minutes. */
const PATIENCE_SECONDS = 120;

/** The durations a file names, with their length in seconds: `const PROMPT: Duration = Duration::from_secs(3);`, `let patience = Duration::from_secs(5);`. */
function durationConstants(code) {
  const unit = { secs: 1, millis: 1e-3, micros: 1e-6, nanos: 1e-9 };
  return new Map(
    [
      ...code.matchAll(
        /\b(?:const|let)\s+(\w+)\s*(?::\s*Duration\s*)?=\s*Duration::from_(secs|millis|micros|nanos)\(\s*([\d_]+)\s*\)/g,
      ),
    ].map((m) => [m[1], Number(m[3].replaceAll('_', '')) * unit[m[2]]]),
  );
}

/** What only a window in which something must not happen does with its wait. */
const NOTHING_CAME = /\.is_err\(\)|\.is_none\(\)/;

/** A file that holds nothing but tests: an integration test, or a `tests.rs` module. */
const TEST_FILE = /(?:^|\/)tests\/|(?:^|\/)tests\.rs$/;

/** `[start, end)` of each `{ ... }` that opens where `pattern` ends. */
function bracedRegions(code, pattern) {
  return [...code.matchAll(pattern)].map((match) => {
    const open = code.indexOf('{', match.index + match[0].length - 1);
    return [open, closingBracket(code, open)];
  });
}

/** Where the test code is: all of a test file, otherwise each module behind `#[cfg(test)]` or `#[cfg(all(test, ..))]`. */
function testRegions(file, code) {
  if (TEST_FILE.test(file)) return [[0, code.length]];
  return bracedRegions(
    code,
    /#\[cfg\((?:all\()?test\b[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{/g,
  );
}

/** The tests on tokio's paused clock: time moves there only when the test lets it, so a bound cannot race the machine. */
function pausedRegions(code) {
  return bracedRegions(code, /#\[tokio::test\([^\]]*start_paused\s*=\s*true[^\]]*\][^{]*\{/g);
}

function inside(regions, index) {
  return regions.some(([start, end]) => index >= start && index < end);
}

/** The comparisons that bound `subject` from above: `subject < x`, `x > subject`, `(a..b).contains(&subject)`. */
function upperBounds(subject) {
  return new RegExp(
    String.raw`(?:${subject})(?:\.\w+\(\))*\s*<=?(?![<=])|[^=\-<>]>=?\s*(?:${subject})|\.\.=?\s*[^)\s][^)]*\)\s*\.contains\(\s*&\s*(?:${subject})`,
  );
}

/** The functions in `code` that return a duration they measured, such as a helper that times a download: what they return counts as measured where they are called. */
function measuringHelpers(code) {
  return [
    ...code.matchAll(/\bfn\s+(\w+)(?=[\s<(])[^{;-]+(?:-(?!>)[^{;-]*)*->[^{;]*Duration[^{;]*\{/g),
  ]
    .filter((match) => {
      const open = match.index + match[0].length - 1;
      return new RegExp(MEASURED).test(code.slice(open, closingBracket(code, open)));
    })
    .map((match) => match[1]);
}

/** The names `body` binds to a measured duration, also from a tuple such as `let (bytes, took) = timed()`. */
function measuredNames(body, helpers) {
  const measured = new RegExp(
    [MEASURED, ...helpers.map((helper) => String.raw`\b${helper}\(`)].join('|'),
  );
  return [...body.matchAll(/let\s+(?:mut\s+)?(\w+|\([^)]*\))\s*(?::[^=;]+)?=([^;]*);/g)]
    .filter((binding) => measured.test(binding[2]))
    .flatMap((binding) => binding[1].match(/\w+/g).filter((name) => name !== 'mut'))
    .map((name) => String.raw`\b${name}\b`);
}

/** The patterns of a short wait in `code`: one given a fixed `Duration` or a short constant, and a deadline a short time away. */
function shortWaits(code) {
  const short = [...durationConstants(code)]
    .filter(([, seconds]) => seconds < PATIENCE_SECONDS)
    .map(([name]) => name);
  // A short duration in place, or a constant of one: `Duration::from_secs(3)`, `PROMPT`.
  const duration = [
    String.raw`Duration::from_\w+\(`,
    ...short.map((name) => String.raw`\b${name}\b`),
  ];
  const waits = [FIXED_WAIT];
  if (short.length > 0) {
    waits.push(String.raw`${WAIT}\(\s*(?:${short.join('|')})\b`);
  }
  return {
    wait: new RegExp(waits.join('|'), 'g'),
    deadline: new RegExp(
      String.raw`let\s+(\w+)\s*=\s*(?:[\w:]+::)?Instant::now\(\)\s*\+\s*(?:${duration.join('|')})`,
      'g',
    ),
  };
}

/** Where in `body` a wait has to succeed in a short time: a short wait not expected to run out, or a deadline a short time away that the test asserts is not yet past (`assert!(Instant::now() < deadline, ..)`). */
function shortWaitsIn(body, { wait, deadline }) {
  const found = [];
  for (const match of body.matchAll(wait)) {
    const statementEnd = body.indexOf(';', match.index);
    const statement = body.slice(match.index, statementEnd === -1 ? body.length : statementEnd);
    if (!NOTHING_CAME.test(statement)) found.push(match.index);
  }
  for (const match of body.matchAll(deadline)) {
    const name = match[1];
    const asserted = new RegExp(
      String.raw`assert!\([^;]*now\(\)\s*<=?\s*${name}\b|assert!\([^;]*\b${name}\s*>=?\s*[\w:]*now\(\)`,
    );
    if (asserted.test(body.slice(match.index))) found.push(match.index);
  }
  return found;
}

/** Where in `body` a sleep is given a duration written in place outside a loop: a window in which nothing must happen is named (`QUIET`), and only a wait loop's poll interval may be written in place. */
function unnamedSleepsIn(body) {
  const loops = bracedRegions(body, /\b(?:while|loop|for)\b[^{;]*\{/g);
  return [...body.matchAll(/\bsleep\(\s*(?:[\w:]+::)?Duration::from_\w+\(/g)]
    .map((match) => match.index)
    .filter((index) => !inside(loops, index));
}

/** Where in `body` a throttled server is set up before the test acts on a run: on a busy machine the transfer ends first. */
function throttledRacesIn(body) {
  const acts = [...body.matchAll(ACTS_ON_A_RUN)].map((match) => match.index);
  const lastAct = Math.max(-1, ...acts);
  return [...body.matchAll(THROTTLED)]
    .map((match) => match.index)
    .filter((index) => index < lastAct);
}

/**
 * The places in one Rust file where a test races the machine, as `{ line, code }`: an upper bound on a measured duration, or a fixed `Duration` written into a wait that has to succeed. A `while` condition, and a wait whose test is that nothing came (`is_err()`, `is_none()`), are windows in which something must not happen, which can only miss a fault, so they are not races. Each statement is read on one line for the bounds, and duration names are read one function at a time.
 */
export function timingRaces(file, text) {
  const code = codeOnly(text);
  const tests = testRegions(file, code);
  if (tests.length === 0) return [];
  const paused = pausedRegions(code);
  const helpers = measuringHelpers(code);
  const short = shortWaits(code);
  const lines = text.split('\n');
  const lineAt = (index) => code.slice(0, index).split('\n').length;
  const found = new Map();
  const report = (index) => found.set(lineAt(index), lines[lineAt(index) - 1].trim());
  for (const [start, end] of bracedRegions(code, /\bfn\s+\w[^{;]*\{/g)) {
    if (!inside(tests, start) || inside(paused, start)) continue;
    const body = code.slice(start, end);
    const bound = upperBounds(
      [String.raw`[\w\]]\.elapsed\(\)(?:\.\w+\(\))*`, ...measuredNames(body, helpers)].join('|'),
    );
    let offset = 0;
    for (const line of body.split('\n')) {
      if (bound.test(line) && !/\bwhile\b/.test(line)) report(start + offset);
      offset += line.length + 1;
    }
    for (const index of [
      ...shortWaitsIn(body, short),
      ...unnamedSleepsIn(body),
      ...throttledRacesIn(body),
    ]) {
      report(start + index);
    }
  }
  return [...found].sort(([a], [b]) => a - b).map(([line, codeLine]) => ({ line, code: codeLine }));
}

function testTiming() {
  const hits = filesMatching(`${MEASURED}|Duration::from_|_bps|_server|throttle`, ['*.rs']).flatMap(
    (file) =>
      timingRaces(file, readFileSync(join(ROOT, file), 'utf8')).map(
        (hit) => `${file}:${String(hit.line)}: ${hit.code}`,
      ),
  );
  if (hits.length > 0) {
    throw new GateError(
      `Tests that race the machine: they bound how long something took from above, or give a wait that has to succeed a short fixed time. A test that takes a second alone takes many times that on a busy CI runner, so it fails there whatever the change (docs/ENGINEERING.md, section 7). Wait for the event under PATIENCE, make the slow path outlast PATIENCE (a held server, a long timeout), count what was sent or tried, or run on tokio's paused clock. A lower bound (it took at least the limit) is fine, and so is a short, named window in which something must not happen (\`sleep(QUIET)\`, \`while start.elapsed() < window\`, a wait expected to time out); a sleep with a duration written in place belongs only in a wait loop, as its poll interval:\n${hits.join('\n')}`,
    );
  }
}

export const TEST_TIMING_CHECKS = { 'test-timing': testTiming };
