import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isInstaller } from './installers.mjs';
import { MANIFEST_FILE, SIGNATURE_FILE } from './update-files.mjs';

const UPDATE_FILES = [MANIFEST_FILE, SIGNATURE_FILE];

export function verifyDraft(manifest, release, updateFiles = []) {
  if (release.draft !== true || release.tag_name !== `v${manifest.version}`) {
    throw new Error('GitHub did not return the expected draft release.');
  }
  const expected = [...manifest.files, ...updateFiles];
  const uploaded = release.assets.filter(
    ({ name }) => isInstaller(name) || UPDATE_FILES.includes(name),
  );
  if (uploaded.length !== expected.length)
    throw new Error('The draft has the wrong number of update assets.');
  for (const { name, size, sha256 } of expected) {
    const matches = uploaded.filter((asset) => asset.name === name);
    const [asset] = matches;
    if (
      matches.length !== 1 ||
      asset.state !== 'uploaded' ||
      asset.size !== size ||
      asset.digest !== `sha256:${sha256}`
    ) {
      throw new Error(`The uploaded asset ${name} does not match the built file.`);
    }
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [inventory, folder, verification] = process.argv.slice(2);
  if (verification !== 'github' && verification !== 'signed') {
    throw new Error('The draft verification mode must be github or signed.');
  }
  const manifest = JSON.parse(readFileSync(inventory, 'utf8'));
  const release = JSON.parse(readFileSync(0, 'utf8'));
  const updateFiles =
    verification === 'signed'
      ? UPDATE_FILES.map((name) => {
          const bytes = readFileSync(join(folder, name));
          return {
            name,
            size: bytes.length,
            sha256: createHash('sha256').update(bytes).digest('hex'),
          };
        })
      : [];
  verifyDraft(manifest, release, updateFiles);
}
