// The Public Suffix List the extension bundles through `tldts` is current enough to release (D35).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { capture, GateError, ROOT } from './process.mjs';

/** The package whose releases carry the list, and the manifest that pins it. */
export const SUFFIX_LIST_PACKAGE = 'tldts';
const EXTENSION_MANIFEST = 'apps/extension/package.json';

/** How long a newer list may have been out before a release must take it. */
export const MAX_DAYS_BEHIND = 60;

/** How long upstream may go without a release before its list is no longer a source to release with. */
export const MAX_DAYS_SILENT = 180;

const DAY_MS = 24 * 60 * 60 * 1000;
const STABLE_VERSION = /^\d+\.\d+\.\d+$/;

/**
 * The registry's stable releases, oldest first, or why they cannot be read.
 * `times` is the registry's `time` field: version to publish date, plus bookkeeping entries.
 * A date that cannot be read is a problem, so a broken registry or mirror never passes a release.
 */
function stableReleases(times) {
  if (typeof times !== 'object' || times === null || Array.isArray(times)) {
    return {
      problem: `The registry's release dates of ${SUFFIX_LIST_PACKAGE} could not be read: it sent ${JSON.stringify(times)}.`,
    };
  }
  const releases = [];
  for (const [version, at] of Object.entries(times)) {
    if (!STABLE_VERSION.test(version)) continue;
    const publishedAt = typeof at === 'string' ? Date.parse(at) : Number.NaN;
    if (Number.isNaN(publishedAt)) {
      return {
        problem: `The registry's release date of ${SUFFIX_LIST_PACKAGE} ${version} could not be read: it sent ${JSON.stringify(at)}.`,
      };
    }
    releases.push({ version, publishedAt });
  }
  return { releases: releases.sort((x, y) => x.publishedAt - y.publishedAt) };
}

/**
 * Why the pinned list is too old to release with, or null when it is fine.
 * The pin is too old when a stable release published after it has been out for more than
 * `maxDays`, or when upstream has published no stable release for more than `maxSilentDays`.
 * How old the pin is by itself does not matter while upstream is publishing: its newest release
 * is the freshest list it offers.
 */
export function staleSuffixList({
  pinned,
  times,
  now,
  maxDays = MAX_DAYS_BEHIND,
  maxSilentDays = MAX_DAYS_SILENT,
}) {
  const { releases, problem } = stableReleases(times);
  if (problem !== undefined) return problem;
  const pin = releases.find(({ version }) => version === pinned);
  if (pin === undefined) {
    return `${SUFFIX_LIST_PACKAGE} ${pinned} is not in the registry's list of releases.`;
  }
  const newest = releases.at(-1) ?? pin;
  if (newest.publishedAt < now.getTime() - maxSilentDays * DAY_MS) {
    const date = new Date(newest.publishedAt).toISOString().slice(0, 10);
    return `There has been no release of ${SUFFIX_LIST_PACKAGE} for more than ${String(maxSilentDays)} days: the newest, ${newest.version}, came out on ${date}. The Public Suffix List changes almost every week, so the list the extension ships falls further behind, and a suffix added since sends SameSite=Strict cookies across sites (D35). Re-evaluate where the list comes from (another library, or a list generated from publicsuffix.org) before releasing.`;
  }
  const deadline = now.getTime() - maxDays * DAY_MS;
  const overdue = releases.filter(
    ({ publishedAt }) => publishedAt > pin.publishedAt && publishedAt < deadline,
  );
  if (overdue.length === 0) return null;
  return `${SUFFIX_LIST_PACKAGE} ${pinned} is more than ${String(maxDays)} days behind ${overdue.at(-1).version}. Its Public Suffix List decides which cookies the extension hands to the app, and an old list sends SameSite=Strict cookies across sites under suffixes added since (D35). Update ${SUFFIX_LIST_PACKAGE} in ${EXTENSION_MANIFEST} to its latest release before releasing.`;
}

/** The exact version the extension pins; a range would let the lockfile, not a reviewed commit, choose the list. */
export function pinnedVersion(manifest) {
  const version = manifest.dependencies?.[SUFFIX_LIST_PACKAGE];
  if (typeof version !== 'string' || !STABLE_VERSION.test(version)) {
    throw new GateError(
      `${EXTENSION_MANIFEST} must pin ${SUFFIX_LIST_PACKAGE} to an exact version (D35); it has ${JSON.stringify(version)}.`,
    );
  }
  return version;
}

function suffixListAge() {
  const manifest = JSON.parse(readFileSync(join(ROOT, EXTENSION_MANIFEST), 'utf8'));
  const pinned = pinnedVersion(manifest);
  const { out } = capture('pnpm', ['view', SUFFIX_LIST_PACKAGE, 'time', '--json'], {
    hint: 'The registry must be reachable to check the age of the Public Suffix List before a release.',
  });
  const problem = staleSuffixList({ pinned, times: JSON.parse(out), now: new Date() });
  if (problem !== null) throw new GateError(problem);
  console.log(`${SUFFIX_LIST_PACKAGE} ${pinned} is current enough to release with.`);
}

export const SUFFIX_LIST_CHECKS = { 'suffix-list': suffixListAge };
