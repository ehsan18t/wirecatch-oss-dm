import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, describe, it } from 'node:test';
import { GateError, ROOT } from '../gate/process.mjs';
import {
  allowedLicences,
  alternatives,
  chooseLicences,
  licencesIn,
  shippedTargets,
} from './licences.mjs';
import { npmPackage } from './npm.mjs';
import { renderNotices, textsOf } from './render.mjs';

const standard = (id) => readFileSync(join(ROOT, 'config', 'licenses', `${id}.txt`), 'utf8');
const NOTICES_TEST = mkdtempSync(join(tmpdir(), 'wirecatch-notices-'));
after(() => rmSync(NOTICES_TEST, { recursive: true, force: true }));

function crate(name, license, files = {}) {
  const dir = join(NOTICES_TEST, name);
  mkdirSync(dir, { recursive: true });
  for (const [file, text] of Object.entries(files)) writeFileSync(join(dir, file), text);
  return { name, version: '1.0.0', license, repository: undefined, authors: ['A Person'], dir };
}

describe('alternatives', () => {
  it('reads SPDX precedence, brackets and the old slash', () => {
    assert.deepEqual(alternatives('MIT/Apache-2.0'), [['MIT'], ['Apache-2.0']]);
    assert.deepEqual(alternatives('MIT OR Apache-2.0 AND ISC'), [['MIT'], ['Apache-2.0', 'ISC']]);
    assert.deepEqual(alternatives('(MIT OR Apache-2.0) AND Unicode-3.0'), [
      ['MIT', 'Unicode-3.0'],
      ['Apache-2.0', 'Unicode-3.0'],
    ]);
    assert.deepEqual(alternatives('Apache-2.0 WITH LLVM-exception OR MIT'), [
      ['Apache-2.0 WITH LLVM-exception'],
      ['MIT'],
    ]);
  });

  it('refuses what it cannot read', () => {
    for (const bad of ['', 'MIT OR', '(MIT', 'MIT Apache-2.0']) {
      assert.throws(() => alternatives(bad), GateError, bad);
    }
  });
});

describe('deny.toml', () => {
  it('gives the allow list and the targets', () => {
    assert.ok(allowedLicences().includes('MIT'));
    assert.ok(shippedTargets().includes('x86_64-pc-windows-msvc'));
  });
});

describe('licencesIn', () => {
  for (const id of ['MIT', 'Apache-2.0', 'BSD-3-Clause', 'ISC', 'MPL-2.0']) {
    it(`knows the standard text of ${id}`, () => {
      assert.deepEqual(licencesIn(standard(id)), [id]);
    });
  }

  it('does not take a pointer for the text', () => {
    assert.deepEqual(licencesIn('Licensed under the Apache License, Version 2.0.'), []);
  });

  it('reads a text written as a source comment', () => {
    const text =
      '// Permission to use, copy, modify, and/or distribute this software for any\n// purpose with or without fee is hereby granted, provided that the above\n// copyright notice and this permission notice appear in all copies.';
    assert.deepEqual(licencesIn(text), ['ISC']);
  });

  it('knows the data agreement', () => {
    const text = '# Community Data License Agreement - Permissive - Version 2.0\n\nThe terms.';
    assert.deepEqual(licencesIn(text), ['CDLA-Permissive-2.0']);
  });
});

describe('chooseLicences', () => {
  it('takes the way the package already ships texts for', () => {
    assert.deepEqual(chooseLicences('MIT OR Apache-2.0', ['Apache-2.0'], ['MIT', 'Apache-2.0']), [
      'Apache-2.0',
    ]);
  });

  it('never takes a way deny.toml does not allow', () => {
    assert.deepEqual(chooseLicences('GPL-2.0-only OR MIT', ['GPL-2.0-only'], ['MIT']), ['MIT']);
    assert.throws(
      () => chooseLicences('GPL-2.0-only', [], ['MIT'], 'some-crate 1.0.0'),
      /some-crate 1\.0\.0 .*allow list/,
    );
  });
});

describe('textsOf', () => {
  const allowed = ['MIT', 'Apache-2.0', 'CDLA-Permissive-2.0'];

  it('adds the standard text for a package that ships none', () => {
    const texts = textsOf(crate('bare', 'MIT'), allowed);
    assert.deepEqual(texts, [{ text: standard('MIT'), standardFor: 'MIT' }]);
  });

  it('keeps a NOTICE and an unknown text, and leaves out the licence not chosen', () => {
    const texts = textsOf(
      crate('both', 'MIT OR Apache-2.0', {
        'LICENSE-MIT': standard('MIT'),
        'LICENSE-APACHE': standard('Apache-2.0'),
        NOTICE: 'Portions by someone else.',
        'LICENSE-other': 'Some other terms.',
        'license.rs': 'not a licence',
      }),
      allowed,
    ).map(({ text }) => text);
    assert.equal(texts.length, 3);
    assert.ok(texts.includes(standard('MIT')));
    assert.ok(texts.includes('Portions by someone else.'));
    assert.ok(texts.includes('Some other terms.'));
  });

  it('finds texts in a licences folder', () => {
    const pkg = crate('foldered', 'MIT');
    mkdirSync(join(pkg.dir, '.licenses'));
    writeFileSync(
      join(pkg.dir, '.licenses', 'Someone-MIT'),
      `Copyright Someone\n${standard('MIT')}`,
    );
    const texts = textsOf(pkg, allowed);
    assert.equal(texts.length, 1);
    assert.equal(texts[0].standardFor, undefined);
  });

  it('carries a data agreement whole', () => {
    const agreement = '# Community Data License Agreement - Permissive - Version 2.0\n\nThe terms.';
    const texts = textsOf(crate('roots', 'CDLA-Permissive-2.0', { LICENSE: agreement }), allowed);
    assert.deepEqual(texts, [{ text: agreement }]);
  });
});

describe('renderNotices', () => {
  it('shows a shared text once, names its holder, and does not depend on the order', () => {
    const packages = [
      crate('zeta', 'MIT', { LICENSE: 'Permission is hereby granted, free of charge. Custom.' }),
      crate('beta', 'MIT'),
      crate('alpha', 'MIT'),
    ];
    const one = renderNotices({ title: 'Title', packages });
    assert.equal(one, renderNotices({ title: 'Title', packages: [...packages].reverse() }));
    assert.equal(one.split('Permission is hereby granted, free of charge, to any').length, 2);
    assert.match(one, /Applies to: alpha 1\.0\.0 \(copyright A Person\), beta 1\.0\.0/);
    assert.match(one, /Public Suffix List snapshot: MPL-2\.0/);
    assert.ok(one.indexOf('alpha 1.0.0: MIT') < one.indexOf('beta 1.0.0: MIT'));
  });

  it('fails when a needed text is missing', () => {
    assert.throws(
      () => renderNotices({ title: 'Title', packages: [crate('compressor', 'Zlib')] }),
      /compressor 1\.0\.0 .*config\/licenses\/Zlib\.txt/,
    );
  });
});

describe('npmPackage', () => {
  it('reads the forms a manifest may take', () => {
    const dir = join(NOTICES_TEST, 'npm');
    mkdirSync(dir);
    writeFileSync(
      join(dir, 'package.json'),
      JSON.stringify({
        name: 'thing',
        version: '2.0.0',
        license: { type: 'MIT' },
        author: { name: 'A Person' },
        repository: { url: 'git+https://example.com/thing.git' },
      }),
    );
    assert.deepEqual(npmPackage(dir), {
      name: 'thing',
      version: '2.0.0',
      license: 'MIT',
      repository: 'https://example.com/thing',
      authors: ['A Person'],
      dir,
    });
  });
});
