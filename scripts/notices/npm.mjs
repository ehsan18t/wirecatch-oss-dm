// The third-party notices of the browser extension (D117), from the npm packages its bundle holds.
//
// Usage: node scripts/notices/npm.mjs OUTPUT_FILE TITLE PACKAGE_FOLDER...
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { GateError, ROOT } from '../gate/process.mjs';
import { renderNotices } from './render.mjs';

/** The icon sets as npm packages ship no licence text, and Lucide's names its holders and the Feather icons it derives from. */
const LICENCE_KEPT_ELSEWHERE = {
  '@iconify-json/lucide': [join(ROOT, 'config', 'licenses', 'lucide.txt')],
};

/** npm allows a bare name, `{ name, email }` or `Name <email> (url)`. */
const personName = (person) => (typeof person === 'string' ? person : person?.name);

/** `git+https://host/path.git`, `ssh://git@host/path`, `owner/name` or `{ url }`, as an address people can open. */
function repositoryUrl(repository) {
  const url = typeof repository === 'string' ? repository : repository?.url;
  if (url && /^[\w.-]+\/[\w.-]+$/.test(url)) return `https://github.com/${url}`;
  return url
    ?.replace(/^git\+/, '')
    .replace(/^ssh:\/\/git@/, 'https://')
    .replace(/\.git$/, '');
}

export function npmPackage(dir) {
  const manifest = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
  const license = manifest.license?.type ?? manifest.license;
  if (!license) throw new GateError(`${manifest.name} ${manifest.version} states no licence.`);
  return {
    name: manifest.name,
    version: manifest.version,
    license,
    repository: repositoryUrl(manifest.repository),
    authors: [manifest.author, ...(manifest.contributors ?? [])].map(personName).filter(Boolean),
    dir,
    textFiles: LICENCE_KEPT_ELSEWHERE[manifest.name] ?? [],
  };
}

if (process.argv[1]?.endsWith('npm.mjs')) {
  const [output, title, ...folders] = process.argv.slice(2);
  try {
    const packages = folders.map(npmPackage);
    writeFileSync(output, renderNotices({ title, packages }));
  } catch (error) {
    if (!(error instanceof GateError)) throw error;
    console.error(error.message);
    process.exit(1);
  }
}
