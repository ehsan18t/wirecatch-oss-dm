import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { test } from 'node:test';
import { GateError } from './process.mjs';
import { runWithTestLimit } from './test-limit.mjs';

const SHORT_MS = 1;
const FOREVER = 'setInterval(() => {}, 1000);';
const NOTICE = 'test a::hangs has been running for over 60 seconds\n';
/** Far past `SHORT_MS`, so a limit left running fires before the fake ends. */
const OUTLASTS_LIMIT = 'setTimeout(() => process.exit(0), 500);';
/** `PATIENCE` in test-support: a killed process can linger as a zombie until it is reaped. */
const PATIENCE_MS = 2 * 60 * 1000;

/** A stand-in for `cargo test`: Node running `script`, which prints what libtest would. */
const fake = (script, options) =>
  runWithTestLimit(process.execPath, ['-e', script], {
    limitMs: SHORT_MS,
    output: { write: () => true },
    ...options,
  });

const say = (text) => `process.stdout.write(${JSON.stringify(text)});`;

/** Starts a child that shares the fake's output and runs on, and prints its process id. */
const startChild = (spawnOptions) => `
  const child = require('node:child_process').spawn(process.execPath, ['-e', ${JSON.stringify(FOREVER)}], ${JSON.stringify(spawnOptions)});
  child.unref();
  console.log('pid ' + child.pid);`;

/** Runs the fake, and returns the process id it printed. */
async function printedPid(run) {
  let printed = '';
  await run({
    write: (chunk) => {
      printed += chunk;
    },
  });
  return Number(/pid (\d+)/.exec(printed)[1]);
}

function isRunning(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function waitUntilGone(pid) {
  const deadline = Date.now() + PATIENCE_MS;
  while (isRunning(pid)) {
    assert.ok(Date.now() < deadline, 'the program under the command is still running');
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}

test('a test that runs out its limit fails the run by name', async () => {
  await assert.rejects(fake(say(NOTICE) + FOREVER), (error) => {
    assert.ok(error instanceof GateError);
    assert.match(error.message, /a::hangs/);
    return true;
  });
});

test('a test with a result in time passes', async () => {
  await fake(`${say(`${NOTICE}test a::hangs ... ok\n`)}process.exit(0);`);
});

test('the end of a test binary forgets its tests, also one whose result line was garbled', async () => {
  await fake(`${say(`${NOTICE}test result: ok. 1 passed\n`)}process.exit(0);`);
});

test('a new test binary forgets the tests of one that aborted', async () => {
  await fake(say(`${NOTICE}running 1 test\n`) + OUTLASTS_LIMIT);
});

for (const mode of ['should panic', 'compile fail', 'compile']) {
  test(`a result that adds "${mode}" to the name counts`, async () => {
    await fake(say(`${NOTICE}test a::hangs - ${mode} ... ok\n`) + OUTLASTS_LIMIT);
  });
}

test('a limit that runs out after the program ended still names the test', async () => {
  const script = `${startChild({ stdio: 'inherit', detached: true })}${say(NOTICE)}process.exit(0);`;
  let pid = 0;
  await assert.rejects(
    fake(script, {
      limitMs: 50,
      drainMs: 500,
      output: {
        write: (chunk) => {
          pid ||= Number(/pid (\d+)/.exec(chunk)?.[1] ?? 0);
        },
      },
    }),
    /a::hangs/,
  );
  process.kill(pid);
});

test(
  'closing the terminal stops the program under the command',
  { skip: process.platform === 'win32' && 'Windows has no SIGHUP to send' },
  async () => {
    const module = new URL('./test-limit.mjs', import.meta.url).href;
    const under = `console.log('pid ' + process.pid);${FOREVER}`;
    const gate = spawn(
      process.execPath,
      [
        '--input-type=module',
        '-e',
        `import { runWithTestLimit } from ${JSON.stringify(module)};
         await runWithTestLimit(process.execPath, ['-e', ${JSON.stringify(under)}]);`,
      ],
      { stdio: ['ignore', 'pipe', 'inherit'] },
    );
    const pid = await new Promise((resolve) => {
      let printed = '';
      gate.stdout.on('data', (chunk) => {
        printed += chunk;
        const found = /pid (\d+)/.exec(printed);
        if (found) resolve(Number(found[1]));
      });
    });
    gate.kill('SIGHUP');
    await waitUntilGone(pid);
  },
);

test('a test name with spaces, as a doctest has, is named whole', async () => {
  const notice =
    'test crates/engine/src/lib.rs - run (line 3) has been running for over 60 seconds\n';
  await assert.rejects(
    fake(say(notice) + FOREVER),
    /crates\/engine\/src\/lib\.rs - run \(line 3\)/,
  );
});

test('a failing run reports its status', async () => {
  await assert.rejects(fake('process.exit(3);'), /failed with status 3/);
});

test('a hang stops the program under the command too', async () => {
  const script = startChild({ stdio: 'inherit' }) + say(NOTICE) + FOREVER;
  const pid = await printedPid((output) => assert.rejects(fake(script, { output }), GateError));
  await waitUntilGone(pid);
});

test('a child left behind holding the output does not hold up the end of the run', async () => {
  const script = `${startChild({ stdio: 'inherit', detached: true })}process.exit(0);`;
  const pid = await printedPid((output) => fake(script, { drainMs: SHORT_MS, output }));
  process.kill(pid);
});
