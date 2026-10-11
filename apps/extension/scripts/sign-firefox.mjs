// Builds the Firefox package and has addons.mozilla.org sign it on the unlisted channel, which
// gives a self-distributed .xpi that release Firefox installs permanently (PUBLISHING.md).
//
// Usage: WEB_EXT_API_KEY=... WEB_EXT_API_SECRET=... pnpm sign:firefox
//
// The AMO credentials come only from those two environment variables: web-ext reads them itself,
// so they never appear on a command line, and config file discovery is turned off, so no file in
// the repository or the home folder can supply them.

import { execFileSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** The environment variables web-ext reads the AMO JWT issuer and secret from. */
export const CREDENTIAL_VARS = ['WEB_EXT_API_KEY', 'WEB_EXT_API_SECRET'];

/**
 * The credential variables that are missing or empty.
 * @param {Record<string, string | undefined>} env
 * @returns {string[]}
 */
export function missingCredentials(env) {
  return CREDENTIAL_VARS.filter((name) => (env[name] ?? '').trim() === '');
}

/**
 * The web-ext arguments that sign the built Firefox package.
 * @param {{ sourceDir: string; artifactsDir: string; sourcesZip: string | null }} paths
 * @returns {string[]}
 */
export function signArgs({ sourceDir, artifactsDir, sourcesZip }) {
  return [
    'sign',
    '--channel=unlisted',
    '--no-config-discovery',
    '--source-dir',
    sourceDir,
    '--artifacts-dir',
    artifactsDir,
    ...(sourcesZip === null ? [] : ['--upload-source-code', sourcesZip]),
  ];
}

function main() {
  const missing = missingCredentials(process.env);
  if (missing.length > 0) {
    console.error(
      `Set ${missing.join(' and ')} to your addons.mozilla.org API key and secret (Tools, Manage API Keys), then run this again. See PUBLISHING.md.`,
    );
    process.exit(1);
  }
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
  const output = join(root, '.output');
  /** Runs a package's command line tool with this Node, without a shell. */
  const run = (bin, args) =>
    execFileSync(process.execPath, [join(root, 'node_modules', ...bin), ...args], {
      cwd: root,
      stdio: 'inherit',
    });

  // Builds .output/firefox-mv3 and zips it with its sources, which AMO asks for because the
  // code is bundled.
  run(['wxt', 'bin', 'wxt.mjs'], ['zip', '-b', 'firefox', '--mv3']);
  const sources = readdirSync(output).find((name) => name.endsWith('-sources.zip'));
  run(
    ['web-ext', 'bin', 'web-ext.js'],
    signArgs({
      sourceDir: join(output, 'firefox-mv3'),
      artifactsDir: join(output, 'signed'),
      sourcesZip: sources === undefined ? null : join(output, sources),
    }),
  );
  console.log(`Signed .xpi saved in ${join(output, 'signed')}`);
}

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
