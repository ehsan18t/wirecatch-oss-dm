// The third-party notices file (D117): the packages a build carries, each with the licence texts it requires, identical texts shown once.
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { GateError, ROOT } from '../gate/process.mjs';
import { allowedLicences, alternatives, chooseLicences, licencesIn } from './licences.mjs';

/** Texts for a package that names a licence without shipping it. */
const CANONICAL_TEXTS = join(ROOT, 'config', 'licenses');

const LICENCE_FILE = /^(?:licen[cs]e|copying|notice|unlicense|copyright)/i;
const NOTICE_FILE = /^notice/i;
const LICENCE_FOLDER = /^\.?licen[cs]es?$/i;
/** Code that happens to be named after the licence, such as a `license.rs`. */
const SOURCE_FILE = /\.(?:rs|toml|json|js|ts|py|sh|c|h|html)$/i;

/** Data built into every release that no package's licence covers: `crates/sites/data` and, through `tldts`, the extension. */
const BUILT_IN_DATA = [
  {
    name: 'Public Suffix List',
    version: 'snapshot',
    license: 'MPL-2.0',
    repository: 'https://publicsuffix.org/list/',
    authors: ['Mozilla Foundation and contributors'],
    dir: null,
  },
];

const RULE = '='.repeat(72);

/** One licence text with what it applies to, as the file sets them out. */
export const noticeSection = (appliesTo, text) =>
  [RULE, `Applies to: ${appliesTo}`, RULE, '', text.trim(), '', ''].join('\n');

function licenceFiles(dir) {
  const found = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      if (LICENCE_FOLDER.test(name))
        found.push(...readdirSync(path).map((inner) => join(path, inner)));
    } else if (LICENCE_FILE.test(name) && !SOURCE_FILE.test(name)) {
      found.push(path);
    }
  }
  return found;
}

function canonicalText(licence, packageName) {
  const file = join(CANONICAL_TEXTS, `${licence}.txt`);
  if (!existsSync(file)) {
    throw new GateError(
      `${packageName} is under ${licence} and ships no text of it. Add it, from the SPDX licence list, as config/licenses/${licence}.txt.`,
    );
  }
  return readFileSync(file, 'utf8');
}

/** The texts one package requires: its own files, and the standard text of each licence it names but does not ship. */
export function textsOf(pkg, allowed) {
  const files = pkg.dir ? licenceFiles(pkg.dir) : [];
  const read = files.map((file) => {
    const text = readFileSync(file, 'utf8');
    return {
      text,
      licences: licencesIn(text),
      isNotice: NOTICE_FILE.test(file.split(/[\\/]/).at(-1)),
    };
  });
  const shipped = read.flatMap((file) => file.licences);
  const label = `${pkg.name} ${pkg.version}`;
  const chosen = chooseLicences(pkg.license, shipped, allowed, label);
  const declined = alternatives(pkg.license)
    .flat()
    .filter((id) => !chosen.includes(id));
  // A file naming only licences not chosen is the other half of an OR. Anything else (an unknown text, a NOTICE, a bundled third party's terms) may be a duty, so it stays.
  const own = read.filter(
    ({ licences, isNotice }) =>
      isNotice || licences.length === 0 || licences.some((id) => !declined.includes(id)),
  );
  const standard = chosen
    .filter((id) => !shipped.includes(id))
    .map((id) => ({ text: canonicalText(id, label), standardFor: id }));
  return [...own.map(({ text }) => ({ text })), ...standard];
}

const normalised = (text) => text.replaceAll(/\s+/g, ' ').trim();

function holders(pkg) {
  return pkg.authors.length > 0 ? pkg.authors.join(', ') : `the ${pkg.name} authors`;
}

/**
 * The file's text, the same for the same packages: nothing in it depends on the machine or the day.
 * `packages` hold `name`, `version`, `license`, `repository`, `authors` and the `dir` with their files.
 */
export function renderNotices({ title, packages }) {
  const allowed = allowedLicences();
  const sorted = [...packages, ...BUILT_IN_DATA].sort(
    (a, b) => a.name.localeCompare(b.name, 'en') || a.version.localeCompare(b.version, 'en'),
  );
  const groups = new Map();
  const lines = sorted.map((pkg) => {
    for (const { text, standardFor } of textsOf(pkg, allowed)) {
      const key = normalised(text);
      if (!groups.has(key)) groups.set(key, { text, users: [] });
      const label = `${pkg.name} ${pkg.version}`;
      // The standard text has a blank for the copyright holder, and a package that does not ship its text states the holder only in its manifest.
      groups.get(key).users.push(standardFor ? `${label} (copyright ${holders(pkg)})` : label);
    }
    return `${pkg.name} ${pkg.version}: ${pkg.license}${pkg.repository ? `, ${pkg.repository}` : ''}`;
  });
  const texts = [...groups.values()].map(({ text, users }) =>
    noticeSection(users.join(', '), text),
  );
  return [
    title,
    '',
    'This software includes the third-party packages listed here. Each stays under its own licence, whose text follows the list. Where a package offers a choice of licences, its text is shown for the licence used here.',
    '',
    ...lines,
    '',
    '',
    ...texts,
  ].join('\n');
}
