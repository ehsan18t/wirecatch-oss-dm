/**
 * @vitest-environment node
 *
 * `pnpm sign:firefox` has addons.mozilla.org sign the Firefox package on the unlisted channel.
 * The AMO credentials may only come from environment variables, never from a file or a command line.
 */
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { CREDENTIAL_VARS, missingCredentials, signArgs } from '../scripts/sign-firefox.mjs';

const root = resolve(__dirname, '..');
const pkg = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8')) as {
  scripts: Record<string, string>;
  devDependencies: Record<string, string>;
};

describe('signing the Firefox package', () => {
  it('signs on the unlisted channel, with config files ignored and no credentials on the command line', () => {
    const args = signArgs({
      sourceDir: 'out/firefox-mv3',
      artifactsDir: 'out/signed',
      sourcesZip: 'out/src.zip',
    });
    expect(args[0]).toBe('sign');
    expect(args).toContain('--channel=unlisted');
    expect(args).toContain('--no-config-discovery');
    expect(args.join(' ')).toContain('--source-dir out/firefox-mv3');
    expect(args.join(' ')).toContain('--upload-source-code out/src.zip');
    expect(args.join(' ')).not.toMatch(/api-key|api-secret|--config\b|--channel=listed/);
    expect(signArgs({ sourceDir: 'a', artifactsDir: 'b', sourcesZip: null })).not.toContain(
      '--upload-source-code',
    );
  });

  it('reads the AMO key and secret from the environment variables web-ext uses', () => {
    expect(CREDENTIAL_VARS).toEqual(['WEB_EXT_API_KEY', 'WEB_EXT_API_SECRET']);
    expect(missingCredentials({})).toEqual(['WEB_EXT_API_KEY', 'WEB_EXT_API_SECRET']);
    expect(missingCredentials({ WEB_EXT_API_KEY: 'user:1:2', WEB_EXT_API_SECRET: ' ' })).toEqual([
      'WEB_EXT_API_SECRET',
    ]);
    expect(missingCredentials({ WEB_EXT_API_KEY: 'user:1:2', WEB_EXT_API_SECRET: 'abc' })).toEqual(
      [],
    );
  });

  it('stops before building, with the fix, when the credentials are not set', () => {
    const env = Object.fromEntries(
      Object.entries(process.env).filter(([name]) => !CREDENTIAL_VARS.includes(name)),
    );
    const result = spawnSync(process.execPath, ['scripts/sign-firefox.mjs'], {
      cwd: root,
      env,
      encoding: 'utf8',
    });
    expect(result.status).toBe(1);
    expect(result.stderr).toContain('Set WEB_EXT_API_KEY and WEB_EXT_API_SECRET');
    expect(result.stdout).toBe('');
  });

  it('is a package script, with web-ext pinned to an exact version', () => {
    // The release check runs first, so no package with a placeholder is signed (D29).
    expect(pkg.scripts['sign:firefox']).toBe(
      'node ../../scripts/gate.mjs publish && node scripts/sign-firefox.mjs',
    );
    expect(pkg.devDependencies['web-ext']).toMatch(/^\d+\.\d+\.\d+$/);
  });
});
