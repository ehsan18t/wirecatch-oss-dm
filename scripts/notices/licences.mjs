// Which licences a package's texts must cover (D117): its SPDX expression narrowed to what deny.toml allows, and which licences a text is.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { GateError, ROOT } from '../gate/process.mjs';

/** A TOML string array in deny.toml, read by hand: CI's supply-chain job has no `pnpm install`, so no TOML parser. */
function denyList(section, key) {
  const toml = readFileSync(join(ROOT, 'deny.toml'), 'utf8').replaceAll(/^\s*#.*$/gm, '');
  const body = toml.split(/^\[/m).find((part) => part.startsWith(`${section}]`));
  const list = body && new RegExp(String.raw`^${key} = \[(.*?)^\]`, 'ms').exec(body)?.[1];
  if (!list) throw new GateError(`deny.toml has no \`${key}\` list in [${section}].`);
  return [...list.matchAll(/"([^"]+)"/g)].map((match) => match[1]);
}

export const allowedLicences = () => denyList('licenses', 'allow');
export const shippedTargets = () => denyList('graph', 'targets');

function tokens(expression) {
  // cargo-deny reads the `/` of old manifests as OR, and so does this.
  return expression
    .replaceAll('/', ' OR ')
    .replaceAll(/([()])/g, ' $1 ')
    .trim()
    .split(/\s+/);
}

/**
 * Every way to satisfy `expression`, each the set of licences that way takes.
 * AND binds tighter than OR, and `X WITH Y` is one licence, as SPDX says.
 */
export function alternatives(expression) {
  const list = tokens(expression);
  let at = 0;

  function atom() {
    if (list[at] === '(') {
      at += 1;
      const inner = anyOf();
      if (list[at] !== ')') throw new GateError(`Unbalanced brackets in "${expression}".`);
      at += 1;
      return inner;
    }
    const name = list[at];
    if (!name || ['AND', 'OR', ')'].includes(name))
      throw new GateError(`Cannot read the licence "${expression}".`);
    at += 1;
    if (list[at] !== 'WITH') return [[name]];
    at += 2;
    return [[`${name} WITH ${list[at - 1]}`]];
  }

  function allOf() {
    let ways = atom();
    while (list[at] === 'AND') {
      at += 1;
      const right = atom();
      ways = ways.flatMap((way) => right.map((other) => [...way, ...other]));
    }
    return ways;
  }

  function anyOf() {
    let ways = allOf();
    while (list[at] === 'OR') {
      at += 1;
      ways = [...ways, ...allOf()];
    }
    return ways;
  }

  const ways = anyOf();
  if (at !== list.length) throw new GateError(`Cannot read the licence "${expression}".`);
  return ways;
}

const NO_FEE = 'permission to use, copy, modify, and/or distribute this software for any purpose';
const MIT_GRANT = 'permission is hereby granted, free of charge';
const KEEP_NOTICE = 'the above copyright notice and this permission notice shall be included';
const BSD_GRANT = 'redistribution and use in source and binary forms';
const APACHE_TERMS = 'terms and conditions for use, reproduction, and distribution';

/** What each licence's full text says. A file that only points at a licence matches none, so the full text still gets added. */
const SIGNATURES = {
  '0BSD': (text) => text.includes(NO_FEE) && !text.includes('above copyright notice'),
  'Apache-2.0': (text) => text.includes(APACHE_TERMS),
  'Apache-2.0 WITH LLVM-exception': (text) =>
    text.includes(APACHE_TERMS) && text.includes('llvm exceptions'),
  'BSD-2-Clause': (text) => text.includes(BSD_GRANT) && !text.includes('neither the name'),
  'BSD-3-Clause': (text) => text.includes(BSD_GRANT) && text.includes('neither the name'),
  'BSL-1.0': (text) => text.includes('boost software license'),
  'CC0-1.0': (text) => text.includes('cc0 1.0 universal') || text.includes('cc0-1.0'),
  'CDLA-Permissive-2.0': (text) =>
    text.includes('community data license agreement') &&
    text.includes('permissive') &&
    text.includes('version 2.0'),
  // deny.toml does not allow it, but a crate offering it as a choice ships its text, which must be recognised to be left out.
  'GPL-2.0-only': (text) =>
    text.includes('gnu general public license') && text.includes('version 2, june 1991'),
  ISC: (text) => text.includes(NO_FEE) && text.includes('above copyright notice'),
  MIT: (text) => text.includes(MIT_GRANT) && text.includes(KEEP_NOTICE),
  'MIT-0': (text) => text.includes(MIT_GRANT) && !text.includes(KEEP_NOTICE),
  'MPL-2.0': (text) => text.includes('mozilla public license') && text.includes('1. definitions'),
  'Unicode-3.0': (text) => text.includes('unicode license'),
  Unlicense: (text) => text.includes('free and unencumbered software'),
  Zlib: (text) =>
    text.includes("provided 'as-is'") && text.includes('altered source versions must be'),
};

/** The licences whose full text is in `text`. */
export function licencesIn(text) {
  // Some crates ship the text as a source comment.
  const uncommented = text.replaceAll(/^[ \t]*(\/{2,}|#+|\*+)[ \t]?/gm, '');
  const plain = uncommented.toLowerCase().replaceAll(/\s+/g, ' ');
  return Object.keys(SIGNATURES).filter((licence) => SIGNATURES[licence](plain));
}

/**
 * The way to satisfy a package's licence that needs the fewest texts it does not ship.
 * Throws when deny.toml allows no way, which `cargo deny` also refuses for a crate.
 */
export function chooseLicences(expression, shipped, allowed, packageName) {
  const ways = alternatives(expression).filter((way) => way.every((id) => allowed.includes(id)));
  if (ways.length === 0) {
    throw new GateError(
      `${packageName} is under "${expression}", which deny.toml's [licenses] allow list does not cover. Replace the package, or check that the licence is compatible with GPL-3.0-only and allow it there.`,
    );
  }
  // Between equals, MIT: its text names the holder, where Apache-2.0's usually leaves it blank.
  const cost = (way) =>
    2 * way.filter((id) => !shipped.includes(id)).length + (way.includes('MIT') ? 0 : 1);
  return ways.reduce((best, way) => (cost(way) < cost(best) ? way : best));
}
