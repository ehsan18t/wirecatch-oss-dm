// Reads the lint attributes in the Rust sources: every `#[allow]` and `#[expect]`, also inside `#[cfg_attr]`.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, ROOT } from './process.mjs';

/** The index of the quote that closes the Rust string opening at `open`. */
function stringEnd(text, open) {
  for (let i = open + 1; i < text.length; i += 1) {
    if (text[i] === '\\') i += 1;
    else if (text[i] === '"') return i;
  }
  return text.length;
}

/** Splits `text` at the commas outside brackets and string literals. */
function splitTopLevel(text) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < text.length; i += 1) {
    const char = text[i];
    if (char === '"') i = stringEnd(text, i);
    else if ('([{'.includes(char)) depth += 1;
    else if (')]}'.includes(char)) depth -= 1;
    else if (char === ',' && depth === 0) {
      parts.push(text.slice(start, i).trim());
      start = i + 1;
    }
  }
  parts.push(text.slice(start).trim());
  return parts.filter(Boolean);
}

/** The index just past the bracket that closes the one at `open`, skipping string literals. */
function closingBracket(text, open) {
  let depth = 0;
  for (let i = open; i < text.length; i += 1) {
    const char = text[i];
    if (char === '"') i = stringEnd(text, i);
    else if ('([{'.includes(char)) depth += 1;
    else if (')]}'.includes(char)) {
      depth -= 1;
      if (depth === 0) return i + 1;
    }
  }
  return text.length;
}

/** `name(args)` split into its name and its top-level arguments, or null for anything else. */
function attributeCall(text) {
  const match = /^([\w:]+)\s*\(/.exec(text);
  if (!match) return null;
  const open = match[0].length - 1;
  return {
    name: match[1],
    args: splitTopLevel(text.slice(open + 1, closingBracket(text, open) - 1)),
  };
}

/** The lints an `allow(...)` or `expect(...)` names, without its `reason`. */
function lintNames(args) {
  return args.filter((arg) => !/^reason\s*=/.test(arg)).sort();
}

/** The attributes of one file: each `#[allow]` and `#[expect]`, and each one a `#[cfg_attr]` switches on. */
function attributesIn(file) {
  const text = readFileSync(join(ROOT, file), 'utf8');
  const found = [];
  for (const start of text.matchAll(/#!?\[/g)) {
    const lineStart = text.lastIndexOf('\n', start.index) + 1;
    if (text.slice(lineStart, start.index).includes('//')) continue;
    const open = start.index + start[0].length - 1;
    const call = attributeCall(text.slice(open + 1, closingBracket(text, open) - 1).trim());
    if (call === null) continue;
    const line = text.slice(0, start.index).split('\n').length;
    const nested =
      call.name === 'cfg_attr' ? call.args.slice(1).map(attributeCall).filter(Boolean) : [call];
    for (const attribute of nested) {
      if (attribute.name === 'allow' || attribute.name === 'expect') {
        found.push({
          file,
          line,
          kind: attribute.name,
          conditional: call.name === 'cfg_attr',
          lints: lintNames(attribute.args),
        });
      }
    }
  }
  return found;
}

let cached = null;

/**
 * Every lint attribute in the Rust sources, with the file and line it is on and the lints it names. Lines that are comments are skipped, so docs may show the syntax. Read once per gate run.
 */
export function rustLintAttributes() {
  cached ??= filesMatching('(allow|expect|cfg_attr)\\s*\\(', ['*.rs']).flatMap(attributesIn);
  return cached;
}
