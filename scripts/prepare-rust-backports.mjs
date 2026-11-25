// Materialize the two temporary Cargo overrides from verified crates.io archives.
import { createHash } from 'node:crypto';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { join, relative, sep } from 'node:path';
import { capture, GateError, ROOT } from './gate/process.mjs';

const PATCHES = join(ROOT, 'patches');
const CACHE = join(ROOT, '.cache', 'rust-backports');
const BACKPORTS = [
  {
    name: 'gpui-pre-windows',
    version: '0.3.14',
    sha256: '8badbaa4129a88e4d2fdaa1615fc61b2a59d4ebb2ac36e5de51f75598f0dd992',
    licences: [],
  },
  {
    name: 'accesskit_windows',
    version: '0.34.0',
    sha256: '106c2b961215864d1c2e703ee63269c25c4e80a577ffb2c1017b9c17dcdf83a1',
    licences: ['LICENSE-APACHE', 'LICENSE-MIT'],
  },
];

async function prepare({ name, version, sha256, licences }) {
  const release = `${name}-${version}`;
  const patch = readFileSync(join(PATCHES, `${release}.patch`));
  const licenceFiles = licences.map((file) => ({
    file,
    bytes: readFileSync(join(PATCHES, `${name}-${file}`)),
  }));
  const hash = createHash('sha256').update(sha256).update(patch);
  for (const { bytes } of licenceFiles) hash.update(bytes);
  const fingerprint = hash.digest('hex').slice(0, 16);
  const destination = join(CACHE, `${release}-${fingerprint}`);
  const manifest = join(destination, 'Cargo.toml');
  if (existsSync(manifest)) return;

  const response = await fetch(`https://static.crates.io/crates/${name}/${release}.crate`, {
    signal: AbortSignal.timeout(30_000),
  });
  if (!response.ok) {
    throw new GateError(`${release} could not be downloaded (HTTP ${String(response.status)}).`);
  }
  const data = Buffer.from(await response.arrayBuffer());
  if (createHash('sha256').update(data).digest('hex') !== sha256) {
    throw new GateError(`${release} does not match its pinned SHA-256.`);
  }

  const scratch = mkdtempSync(join(CACHE, '.prepare-'));
  try {
    const archive = join(scratch, `${release}.crate`);
    writeFileSync(archive, data);
    capture('tar', ['-xf', archive, '-C', scratch]);
    const source = join(scratch, release);
    const fromRoot = relative(ROOT, source).split(sep).join('/');
    const stagedPatch = join(scratch, 'backport.patch');
    writeFileSync(stagedPatch, patch);
    capture('git', ['apply', '--whitespace=nowarn', `--directory=${fromRoot}`, stagedPatch]);
    for (const { file, bytes } of licenceFiles) writeFileSync(join(source, file), bytes);

    // Publish a complete copy. Another setup may have published the same files first.
    try {
      renameSync(source, destination);
      console.log(`Prepared ${relative(ROOT, destination).split(sep).join('/')}.`);
    } catch (error) {
      if (!existsSync(manifest)) throw error;
    }
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

try {
  mkdirSync(CACHE, { recursive: true });
  for (const backport of BACKPORTS) await prepare(backport);
} catch (error) {
  console.error(`Preparing Rust backports failed. ${String(error)}`);
  process.exitCode = 1;
}
