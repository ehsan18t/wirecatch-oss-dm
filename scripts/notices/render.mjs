// The third-party notices file (D117): the packages a build carries, each with the licence texts it requires, identical texts shown once.
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { basename, join } from 'node:path';
import { GateError, ROOT } from '../gate/process.mjs';
import { allowedLicences, alternatives, chooseLicences, licencesIn } from './licences.mjs';

/** Texts for a package that names a licence without shipping it. */
const CANONICAL_TEXTS = join(ROOT, 'config', 'licenses');

const LICENCE_FILE = /^(?:licen[cs]e|copying|notice|unlicense|copyright)/i;
const NOTICE_FILE = /^notice/i;
const LICENCE_FOLDER = /^\.?licen[cs]es?$/i;
/** A file that is only the text of the licence its name states, so it can be left out when another is chosen. A name like `LICENSE-THIRD-PARTY` is someone else's terms. */
const PLAIN_LICENCE_NAME =
  /^(?:licen[cs]e|copying|unlicense)(?:[-_.](?:mit|apache[-_.v\d]*|bsd[-_.v\d]*|0bsd|isc|zlib|bsl[-_.v\d]*|boost|cc0[-_.v\d]*|mpl[-_.v\d]*|gpl[-_.v\d]*|unlicense|txt|md|rst))*$/i;
/** What a package ships but no build compiles, so its licences do not reach the build. */
const UNBUILT_FOLDER =
  /^(?:(?:[\w.]+-)*tests?|examples?|benches|fuzz|fixtures?|test-?data|target|node_modules|\.git(?:hub)?)$/i;
/** Not a licence, whatever its name: code, or an icon such as `copyright.svg`. */
const SOURCE_FILE = /\.(?:rs|toml|json|js|ts|py|sh|c|h|html|svg|png)$/i;

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

/** A package's licence files. `vendored` ones, deeper in the package, are the terms of code it copied in. */
function licenceFiles(dir, vendored = false) {
  const found = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      if (LICENCE_FOLDER.test(name)) {
        found.push(...readdirSync(path).map((inner) => ({ path: join(path, inner), vendored })));
      } else if (!UNBUILT_FOLDER.test(name)) {
        found.push(...licenceFiles(path, true));
      }
    } else if (LICENCE_FILE.test(name) && !SOURCE_FILE.test(name)) {
      found.push({ path, vendored });
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
  const files = [
    ...(pkg.dir ? licenceFiles(pkg.dir) : []),
    ...(pkg.textFiles ?? []).map((path) => ({ path, vendored: false })),
  ];
  const read = files.map(({ path, vendored }) => {
    const text = readFileSync(path, 'utf8');
    const name = basename(path);
    return {
      text,
      vendored,
      licences: licencesIn(text),
      // A NOTICE, or a file named for more than a licence, may be a duty whatever it contains.
      mustStay: vendored || NOTICE_FILE.test(name) || !PLAIN_LICENCE_NAME.test(name),
    };
  });
  // Vendored terms are not the package's own, so they do not stand in for its licence.
  const shipped = read.filter((file) => !file.vendored).flatMap((file) => file.licences);
  const label = `${pkg.name} ${pkg.version}`;
  const chosen = chooseLicences(pkg.license, shipped, allowed, label);
  const declined = alternatives(pkg.license)
    .flat()
    .filter((id) => !chosen.includes(id));
  // A plain file naming only licences not chosen is the other half of an OR. An unknown text may be a duty, so it stays.
  const own = read.filter(
    ({ licences, mustStay }) =>
      mustStay || licences.length === 0 || licences.some((id) => !declined.includes(id)),
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
    const where = pkg.repository ? `, ${pkg.repository}` : '';
    const who = pkg.authors.length > 0 ? `, by ${pkg.authors.join(', ')}` : '';
    return `${pkg.name} ${pkg.version}: ${pkg.license}${where}${who}`;
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
