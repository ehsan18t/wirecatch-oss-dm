// Reads the lint attributes in the Rust sources: every `#[allow]` and `#[expect]`, also inside `#[cfg_attr]`, however deeply nested. Its reader for Rust code without comments and literals serves the other source checks too.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { filesMatching, ROOT } from './process.mjs';

/** Where a raw string starts (`r"`, `r#"`, `br##"`, `cr#"`): its `#`s close it again after the closing quote. */
const RAW_STRING = /^[bc]?r(#*)"/;
/** A character literal (`'"'`, `'\''`, `'\u{2014}'`), told apart from a lifetime by its closing quote. */
const CHAR_LITERAL = /^'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'\n])'/;

/** `text` with every character other than a line break turned into a space, so positions and line numbers stay. */
function blank(text) {
  return text.replaceAll(/[^\n]/g, ' ');
}

/** Where the comment, string or character literal starting at `i` ends, or -1 when none starts there. */
function literalEnd(text, i) {
  const rest = text.slice(i, i + 64);
  if (rest.startsWith('//')) {
    const end = text.indexOf('\n', i);
    return end === -1 ? text.length : end;
  }
  if (rest.startsWith('/*')) return blockCommentEnd(text, i);
  const raw = RAW_STRING.exec(rest);
  if (raw && (i === 0 || !/\w/.test(text[i - 1]))) {
    const end = text.indexOf(`"${raw[1]}`, i + raw[0].length);
    return end === -1 ? text.length : end + 1 + raw[1].length;
  }
  if (text[i] === '"') return quotedEnd(text, i);
  const char = CHAR_LITERAL.exec(rest);
  return char ? i + char[0].length : -1;
}

/** Where the block comment opening at `open` ends; block comments nest in Rust. */
function blockCommentEnd(text, open) {
  let depth = 0;
  for (let i = open; i < text.length - 1; i += 1) {
    const pair = text.slice(i, i + 2);
    if (pair === '/*') depth += 1;
    else if (pair === '*/') depth -= 1;
    if (pair === '/*' || pair === '*/') i += 1;
    if (depth === 0) return i + 1;
  }
  return text.length;
}

/** Where the string opening with the quote at `open` ends, past its closing quote. */
function quotedEnd(text, open) {
  for (let i = open + 1; i < text.length; i += 1) {
    if (text[i] === '\\') i += 1;
    else if (text[i] === '"') return i + 1;
  }
  return text.length;
}

/** `text` with its comments, strings and character literals blanked, so only code is left to read. */
export function codeOnly(text) {
  const pieces = [];
  let kept = 0;
  let i = 0;
  while (i < text.length) {
    const end = literalEnd(text, i);
    if (end === -1) {
      i += 1;
    } else {
      pieces.push(text.slice(kept, i), blank(text.slice(i, end)));
      kept = end;
      i = end;
    }
  }
  pieces.push(text.slice(kept));
  return pieces.join('');
}

/** The index just past the bracket that closes the one at `open`. */
export function closingBracket(code, open) {
  let depth = 0;
  for (let i = open; i < code.length; i += 1) {
    if ('([{'.includes(code[i])) depth += 1;
    else if (')]}'.includes(code[i])) {
      depth -= 1;
      if (depth === 0) return i + 1;
    }
  }
  return code.length;
}

/** Splits `code` at the commas outside brackets. */
function splitTopLevel(code) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < code.length; i += 1) {
    if ('([{'.includes(code[i])) depth += 1;
    else if (')]}'.includes(code[i])) depth -= 1;
    else if (code[i] === ',' && depth === 0) {
      parts.push(code.slice(start, i).trim());
      start = i + 1;
    }
  }
  parts.push(code.slice(start).trim());
  return parts.filter(Boolean);
}

/** `name(args)` split into its name and its top-level arguments, or null for anything else. */
function attributeCall(code) {
  const match = /^([\w:]+)\s*\(/.exec(code);
  if (!match) return null;
  const open = match[0].length - 1;
  return {
    name: match[1],
    args: splitTopLevel(code.slice(open + 1, closingBracket(code, open) - 1)),
  };
}

/** The `allow` and `expect` attributes `call` is or switches on, through any depth of `cfg_attr`. */
function lintAttributes(call, conditional) {
  if (call === null) return [];
  if (call.name === 'cfg_attr')
    return call.args.slice(1).flatMap((arg) => lintAttributes(attributeCall(arg), true));
  if (call.name !== 'allow' && call.name !== 'expect') return [];
  // The reason is a string, blanked with the other literals, so only `reason =` is left of it.
  // A `$lint` or `$($lint),*` inside `macro_rules!` is filled in by each use of the macro, which this reader does not expand; those lints are left to review. The comma splits a repetition, leaving its operator (`*`, `+`, `?`) as an argument of its own.
  const lints = call.args
    .filter((arg) => !/^reason\s*=/.test(arg) && !arg.startsWith('$') && !/^[*+?]$/.test(arg))
    .sort();
  if (lints.length === 0) return [];
  return [{ kind: call.name, conditional, lints }];
}

/** The lint attributes of one file. */
function attributesIn(file) {
  const code = codeOnly(readFileSync(join(ROOT, file), 'utf8'));
  return [...code.matchAll(/#!?\[/g)].flatMap((start) => {
    const open = start.index + start[0].length - 1;
    const call = attributeCall(code.slice(open + 1, closingBracket(code, open) - 1).trim());
    const line = code.slice(0, start.index).split('\n').length;
    return lintAttributes(call, false).map((attribute) => ({ file, line, ...attribute }));
  });
}

let cached = null;

/**
 * Every lint attribute in the Rust sources, with the file and line it is on and the lints it names. Comments and string literals are not read, so docs and tests may show the syntax. Read once per gate run.
 */
export function rustLintAttributes() {
  cached ??= filesMatching(String.raw`(allow|expect|cfg_attr)\s*\(`, ['*.rs']).flatMap(
    attributesIn,
  );
  return cached;
}
