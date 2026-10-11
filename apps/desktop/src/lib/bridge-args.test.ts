// The bridge that ships trusts only the store extensions; only development builds also trust
// the unpacked development extension.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { bridgeCargoArgs } from '../../scripts/bridge-args.mjs';

describe('building the bridge for bundling', () => {
  it('turns on dev-extension for debug builds only', () => {
    expect(bridgeCargoArgs(false)).toEqual([
      'build',
      '-p',
      'wirecatch-bridge',
      '--features',
      'dev-extension',
    ]);
    const release = bridgeCargoArgs(true);
    expect(release).toContain('--release');
    expect(release.join(' ')).not.toMatch(/dev-extension|--features|--all-features/);
  });

  it('builds a release bridge for the release bundle', () => {
    // Vitest runs from apps/desktop.
    const pkg = JSON.parse(readFileSync(resolve(process.cwd(), 'package.json'), 'utf8')) as {
      scripts: Record<string, string>;
    };
    expect(pkg.scripts.bundle).toMatch(/prepare-bridge\.mjs --release/);
    expect(pkg.scripts.bundle).not.toMatch(/--debug/);
  });
});
