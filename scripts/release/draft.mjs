import { createHash } from 'node:crypto';
import { createReadStream, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { MANIFEST_FILE, SIGNATURE_FILE } from './update-files.mjs';

/** GitHub can list an upload before it has computed the asset's digest. */
export const DIGEST_READS = 10;
const DIGEST_RETRY_MS = 3000;

function checkDraft(version, release) {
  if (
    release.draft !== true ||
    release.tag_name !== `v${version}` ||
    release.prerelease !== version.includes('-')
  ) {
    throw new Error('GitHub did not return the expected draft release.');
  }
}

/** Existing assets must match before a retry can add the files an interrupted upload missed. */
function missingAssets(expected, uploaded) {
  const missing = new Map(expected.map((file) => [file.name, file]));
  for (const asset of uploaded) {
    const file = missing.get(asset.name);
    if (
      !file ||
      asset.state !== 'uploaded' ||
      asset.size !== file.size ||
      asset.digest !== `sha256:${file.sha256}`
    ) {
      throw new Error(`The uploaded asset ${asset.name} does not match the built file.`);
    }
    missing.delete(asset.name);
  }
  return [...missing.values()];
}

function digestPending(expected, uploaded) {
  return uploaded.some(
    (asset) =>
      !asset.digest &&
      asset.state === 'uploaded' &&
      expected.some(({ name, size }) => name === asset.name && size === asset.size),
  );
}

function fileMetadata(folder, name) {
  const bytes = readFileSync(join(folder, name));
  return { name, size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') };
}

function checkLocal(folder, { name, size, sha256 }) {
  const local = fileMetadata(folder, name);
  if (local.size !== size || local.sha256 !== sha256) {
    throw new Error(`The local installer ${name} differs from the prepared inventory.`);
  }
}

/** The files the draft must hold: the inventory's installers, the signed files and the extension ZIPs. */
export function prepareDraft({ inventory, folder, verification, tag }) {
  if (verification !== 'github' && verification !== 'signed') {
    throw new Error('The draft verification mode must be github or signed.');
  }
  const bytes = readFileSync(inventory);
  const manifest = JSON.parse(bytes.toString('utf8'));
  const { version } = manifest;
  if (tag !== `v${version}`) throw new Error('The inventory does not match the current tag.');
  const signedFiles = verification === 'signed' ? [MANIFEST_FILE, SIGNATURE_FILE] : [];
  if (signedFiles.length && !bytes.equals(readFileSync(join(folder, MANIFEST_FILE)))) {
    throw new Error('The signed manifest differs from the prepared installer inventory.');
  }
  const files = [...manifest.files, ...signedFiles.map((name) => fileMetadata(folder, name))];
  manifest.files.forEach((installer) => checkLocal(folder, installer));
  const prepared = new Set(files.map(({ name }) => name));
  for (const name of readdirSync(folder)) {
    if (name.endsWith('.zip')) files.push(fileMetadata(folder, name));
    else if (!prepared.has(name)) throw new Error(`Unexpected release file: ${name}`);
  }
  return { version, files, prerelease: version.includes('-') };
}

/** A workflow rerun keeps its run ID. A separate run or a maintainer's draft needs a human decision. */
export async function stageDraft(api, { version, files, runId, commit }, wait = delay) {
  const marker = `<!-- release-workflow ${runId} ${commit} -->`;
  const releases = await api.forTag();
  if (releases.length > 1)
    throw new Error('Multiple releases have this tag. Review them on GitHub.');
  const release = releases[0] ?? (await api.create(marker));
  const { id } = release;
  const missingNow = async () => {
    for (let read = 1; ; read++) {
      const draft = await api.read(id);
      checkDraft(version, draft);
      if (draft.id !== id || !draft.body?.split('\n').includes(marker)) {
        throw new Error('This draft belongs to another run. Review it on GitHub.');
      }
      if (!digestPending(files, draft.assets)) return missingAssets(files, draft.assets);
      if (read === DIGEST_READS) {
        throw new Error('GitHub has not reported the SHA-256 of an uploaded asset. Rerun the job.');
      }
      await wait(DIGEST_RETRY_MS);
    }
  };
  let missing = await missingNow();
  for (const file of files) {
    if (missing.some(({ name }) => name === file.name)) {
      await api.upload(id, file);
      missing = await missingNow();
    }
  }
  if (missing.length) throw new Error('The verified draft is missing build files.');
  return id;
}

export function github({ repository, token, tag, commit, prerelease, folder }) {
  const request = async (url, options) => {
    const response = await fetch(url, {
      ...options,
      redirect: 'error',
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: 'application/vnd.github+json',
        ...options?.headers,
      },
    });
    if (!response.ok)
      throw new Error(`GitHub returned ${response.status} for ${options?.method ?? 'GET'} ${url}.`);
    return response.json();
  };
  const repo = `https://api.github.com/repos/${repository}`;
  const releases = `${repo}/releases`;
  const post = (url, data) =>
    request(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(data),
    });
  return {
    async forTag() {
      const tagged = await request(`${repo}/commits/tags/${encodeURIComponent(tag)}`);
      if (tagged.sha !== commit) throw new Error('The tag no longer points to the build commit.');
      const matches = [];
      const perPage = 100;
      for (let page = 1; ; page++) {
        const batch = await request(`${releases}?per_page=${perPage}&page=${page}`);
        matches.push(...batch.filter((release) => release.tag_name === tag));
        if (batch.length < perPage) return matches;
      }
    },
    async create(marker) {
      const notes = await post(`${releases}/generate-notes`, {
        tag_name: tag,
        target_commitish: commit,
      });
      return post(releases, {
        tag_name: tag,
        target_commitish: commit,
        name: tag,
        draft: true,
        prerelease,
        body: `${notes.body}\n\n${marker}`,
      });
    },
    read: (id) => request(`${releases}/${id}`),
    upload: (id, file) =>
      request(
        `https://uploads.github.com/repos/${repository}/releases/${id}/assets?name=${encodeURIComponent(file.name)}`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/octet-stream',
            'Content-Length': String(file.size),
          },
          body: createReadStream(join(folder, file.name)),
          duplex: 'half',
        },
      ),
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [inventory, folder, verification, commit] = process.argv.slice(2);
  const {
    GITHUB_TOKEN: token,
    GITHUB_REPOSITORY: repository,
    GITHUB_REF_NAME: tag,
    GITHUB_RUN_ID: runId,
  } = process.env;
  if (!token || !repository || !tag || !commit || !runId)
    throw new Error('Missing release workflow inputs.');
  const { version, files, prerelease } = prepareDraft({ inventory, folder, verification, tag });
  const api = github({ repository, token, tag, commit, prerelease, folder });
  const id = await stageDraft(api, { version, files, runId, commit });
  console.log(`Verified GitHub release ID ${id} for ${tag}.`);
}
