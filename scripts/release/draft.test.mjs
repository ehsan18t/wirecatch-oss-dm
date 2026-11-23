import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { DIGEST_READS, github, prepareDraft, stageDraft } from './draft.mjs';
import { MANIFEST_FILE, SIGNATURE_FILE } from './update-files.mjs';

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

/** A built release folder with two installers and the extension, and its inventory beside it. */
function built(t, version) {
  const root = mkdtempSync(join(tmpdir(), 'release-draft-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const folder = join(root, 'release');
  mkdirSync(folder);
  const installers = ['App-setup.exe', 'App.AppImage'].map((name) => {
    writeFileSync(join(folder, name), name);
    return { name, size: name.length, sha256: sha256(name) };
  });
  writeFileSync(join(folder, 'extension.zip'), 'extension');
  const inventory = join(root, 'release-manifest.json');
  writeFileSync(inventory, JSON.stringify({ version, files: installers }));
  const prepare = (verification, tag = `v${version}`) =>
    prepareDraft({ inventory, folder, verification, tag });
  return { folder, inventory, installers, prepare };
}

test('a draft holds the inventory installers and the extension ZIPs of its tag', (t) => {
  const { folder, installers, prepare } = built(t, '1.2.0-beta.1');
  const zip = { name: 'extension.zip', size: 9, sha256: sha256('extension') };
  assert.deepEqual(prepare('github'), {
    version: '1.2.0-beta.1',
    files: [...installers, zip],
    prerelease: true,
  });
  assert.equal(built(t, '1.2.0').prepare('github').prerelease, false);
  assert.throws(() => prepare(''), /github or signed/);
  assert.throws(() => prepare('github', 'v1.2.0'), /does not match the current tag/);

  writeFileSync(join(folder, 'notes.txt'), 'not a release file');
  assert.throws(() => prepare('github'), /Unexpected release file: notes\.txt/);
  rmSync(join(folder, 'notes.txt'));
  writeFileSync(join(folder, installers[0].name), 'rebuilt');
  assert.throws(() => prepare('github'), /local installer App-setup\.exe differs/);
});

test('a signed draft adds the signed files, which must hold the inventory bytes', (t) => {
  const { folder, inventory, installers, prepare } = built(t, '1.2.0');
  copyFileSync(inventory, join(folder, MANIFEST_FILE));
  writeFileSync(join(folder, SIGNATURE_FILE), 'signature');
  const names = prepare('signed').files.map(({ name }) => name);
  assert.deepEqual(
    names.sort(),
    [...installers.map(({ name }) => name), 'extension.zip', MANIFEST_FILE, SIGNATURE_FILE].sort(),
  );
  assert.throws(() => prepare('github'), new RegExp(`Unexpected release file: ${MANIFEST_FILE}`));
  writeFileSync(join(folder, MANIFEST_FILE), 'another inventory');
  assert.throws(() => prepare('signed'), /signed manifest differs/);
});

function fixture() {
  const tag = 'v1.2.0';
  const version = '1.2.0';
  const files = ['app.exe', 'extension.zip'].map((name) => ({
    name,
    sha256: 'a'.repeat(64),
    size: 4,
  }));
  const release = { id: 41, tag_name: tag, draft: true, prerelease: false, assets: [] };
  const state = { drafts: [], uploads: [], creates: 0, reads: [], waits: 0 };
  const api = {
    forTag: () => state.drafts,
    create: (body) => {
      state.creates++;
      state.drafts.push({ ...release, body });
      return state.drafts[0];
    },
    read: (id) => {
      state.reads.push(id);
      return state.drafts.find((draft) => draft.id === id);
    },
    upload: (id, file) => {
      assert.equal(id, release.id);
      state.uploads.push(file.name);
      state.drafts[0].assets.push({ ...file, state: 'uploaded', digest: `sha256:${file.sha256}` });
    },
  };
  return {
    api,
    release,
    state,
    run: () =>
      stageDraft(api, { version, files, runId: '23', commit: 'abc' }, async () => {
        state.waits++;
      }),
  };
}

test('a retry verifies the same draft and uploads only missing build files', async () => {
  const { api, run, state } = fixture();
  const upload = api.upload;
  api.upload = (id, file) => {
    if (state.uploads.length) throw new Error('upload interrupted');
    upload(id, file);
  };
  await assert.rejects(run, /upload interrupted/);
  api.upload = upload;
  assert.equal(await run(), 41);
  assert.equal(await run(), 41);
  assert.equal(state.creates, 1);
  assert.deepEqual(state.uploads, ['app.exe', 'extension.zip']);
  assert.ok(state.reads.length > 0 && state.reads.every((id) => id === 41));
});

test('published, foreign, ambiguous, and changed drafts stop before another upload', async () => {
  for (const change of [
    (draft) => {
      draft.draft = false;
    },
    (draft) => {
      draft.body = 'A maintainer draft';
    },
    (draft, drafts) => {
      drafts.push({ ...draft, id: 97 });
    },
    (draft) => {
      draft.assets[0].digest = `sha256:${'b'.repeat(64)}`;
    },
    (draft) => {
      draft.assets[0].size = 5;
    },
    (draft) => {
      draft.assets[0].state = 'starter';
    },
    (draft) => {
      draft.assets[0].name = 'different.deb';
    },
    (draft) => {
      draft.prerelease = true;
    },
  ]) {
    const { run, state } = fixture();
    await run();
    state.drafts[0].assets.pop();
    change(state.drafts[0], state.drafts);
    await assert.rejects(run);
    assert.equal(state.creates, 1);
    assert.equal(state.uploads.length, 2);
    assert.equal(state.waits, 0);
  }
});

test('an upload whose digest GitHub reports late is read again until it appears', async () => {
  const { api, run, state } = fixture();
  const upload = api.upload;
  const pending = [];
  api.upload = (id, file) => {
    upload(id, file);
    const asset = state.drafts[0].assets.at(-1);
    pending.push({ asset, digest: asset.digest });
    asset.digest = null;
  };
  const read = api.read;
  api.read = (id) => {
    if (state.reads.length % 2 === 0) {
      for (const { asset, digest } of pending.splice(0)) asset.digest = digest;
    }
    return read(id);
  };
  assert.equal(await run(), 41);
  assert.deepEqual(state.uploads, ['app.exe', 'extension.zip']);
  assert.equal(state.waits, 2);
});

test('an upload whose digest never appears stops the draft', async () => {
  const { api, run, state } = fixture();
  const upload = api.upload;
  api.upload = (id, file) => {
    upload(id, file);
    state.drafts[0].assets.at(-1).digest = null;
  };
  await assert.rejects(run, /has not reported the SHA-256/);
  assert.deepEqual(state.uploads, ['app.exe']);
  assert.equal(state.waits, DIGEST_READS - 1);
});

test('publication during an interrupted draft stops the remaining uploads', async () => {
  const { api, run, state } = fixture();
  const upload = api.upload;
  api.upload = (id, file) => {
    upload(id, file);
    state.drafts[0].draft = false;
  };
  await assert.rejects(run, /draft release/);
  assert.deepEqual(state.uploads, ['app.exe']);
});

test('GitHub receives the build bytes at the release ID returned by draft creation', async (t) => {
  const folder = mkdtempSync(join(tmpdir(), 'release-upload-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const name = 'app.exe';
  const bytes = Buffer.from('build');
  writeFileSync(join(folder, name), bytes);
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  let draft;
  const api = 'https://api.github.com/repos/owner/repo';
  const uploads = 'https://uploads.github.com/repos/owner/repo';
  t.mock.method(globalThis, 'fetch', async (url, options) => {
    assert.equal(options.redirect, 'error');
    assert.equal(options.headers.Authorization, 'Bearer fixture');
    if (url === `${api}/commits/tags/v1.2.0-beta.1`) return Response.json({ sha: 'abc' });
    if (url === `${api}/releases?per_page=100&page=1`) return Response.json([]);
    if (url === `${api}/releases/generate-notes`) return Response.json({ body: 'Release notes.' });
    if (url === `${api}/releases` && options.method === 'POST') {
      draft = { ...JSON.parse(options.body), id: 71, assets: [] };
      assert.ok(draft.body.startsWith('Release notes.'));
      return Response.json(draft);
    }
    if (url === `${uploads}/releases/71/assets?name=app.exe`) {
      assert.deepEqual(Buffer.from(await new Response(options.body).arrayBuffer()), bytes);
      assert.equal(options.headers['Content-Length'], String(bytes.length));
      draft.assets.push({
        name,
        size: bytes.length,
        digest: `sha256:${sha256}`,
        state: 'uploaded',
      });
      return Response.json(draft.assets[0]);
    }
    assert.equal(url, `${api}/releases/71`);
    return Response.json(draft);
  });
  const client = github({
    repository: 'owner/repo',
    token: 'fixture',
    tag: 'v1.2.0-beta.1',
    commit: 'abc',
    prerelease: true,
    folder,
  });
  assert.equal(
    await stageDraft(client, {
      version: '1.2.0-beta.1',
      files: [{ name, size: bytes.length, sha256 }],
      runId: '56',
      commit: 'abc',
    }),
    71,
  );
});
