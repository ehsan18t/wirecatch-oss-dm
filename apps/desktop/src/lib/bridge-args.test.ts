// The bridge trusts the same extensions in every build (D22), so bundling never turns on a cargo
// feature; its only feature, test-overrides, must never ship.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { bridgeCargoArgs } from '../../scripts/bridge-args.mjs';

describe('building the bridge for bundling', () => {
  it('turns on no cargo feature in either profile', () => {
    expect(bridgeCargoArgs(false)).toEqual(['build', '-p', 'wirecatch-bridge']);
    expect(bridgeCargoArgs(true)).toEqual(['build', '-p', 'wirecatch-bridge', '--release']);
    for (const release of [false, true]) {
      expect(bridgeCargoArgs(release).join(' ')).not.toMatch(/--features|--all-features|-F\b/);
    }
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
