// `task-span`: refuses a task or blocking work started with tokio's own functions in the shipped code of the crates a download's work runs in. They start it in no span, so the log filter that keeps a private download's lines out of the log (D279) would let that task's lines through; `wirecatch_engine::task` keeps the caller's span.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, GateError, ROOT } from './process.mjs';
import { codeOnly } from './rust-attributes.mjs';
import { declaredTestModules, isTestFile, testItems } from './test-code.mjs';

/** The one file that may call tokio's functions: it wraps them. */
const WRAPPER = 'crates/engine/src/task.rs';
/** Where a download's work runs, and so where a private download's lines come from. */
const DOWNLOAD_CRATES = ['engine', 'streams', 'media', 'core'].map(
  (name) => `crates/${name}/src/*.rs`,
);
const BARE = /\btokio::(?:task::)?(?:spawn|spawn_blocking)\s*\(/g;

/** The lines of one Rust file, outside its tests, that start a task without the caller's span, as `{ line, code }`. */
export function unwrappedSpawns(text) {
  const code = codeOnly(text);
  const tests = testItems(code);
  const lines = text.split('\n');
  return [...code.matchAll(BARE)]
    .filter((match) => tests.every(([start, end]) => match.index < start || match.index >= end))
    .map((match) => {
      const line = code.slice(0, match.index).split('\n').length;
      return { line, code: lines[line - 1].trim() };
    });
}

function taskSpan() {
  const testModules = declaredTestModules();
  const hits = filesMatching('tokio::(task::)?spawn', DOWNLOAD_CRATES)
    .filter((file) => file !== WRAPPER && !isTestFile(file, testModules))
    .flatMap((file) =>
      unwrappedSpawns(readFileSync(join(ROOT, file), 'utf8')).map(
        (hit) => `${file}:${String(hit.line)}: ${hit.code}`,
      ),
    );
  if (hits.length > 0) {
    throw new GateError(
      `Tasks started outside the caller's span, which the log filter for private downloads cannot see (D279). Use \`wirecatch_engine::task::spawn\` or \`spawn_blocking\`:\n${hits.join('\n')}`,
    );
  }
}

export const TASK_SPAN_CHECKS = { 'task-span': taskSpan };
