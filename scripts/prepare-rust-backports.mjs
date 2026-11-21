// Materialize the two temporary Cargo overrides from verified crates.io archives.
import { createHash } from 'node:crypto';
import {
  copyFileSync,
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
    version: '0.3.12',
    sha256: '7201762420222d5b5ce5bdf0056996199212a3349bfb2457a3a0238c688681f3',
    licences: [],
  },
  {
    name: 'accesskit_windows',
    version: '0.34.0',
    sha256: '106c2b961215864d1c2e703ee63269c25c4e80a577ffb2c1017b9c17dcdf83a1',
    licences: ['LICENSE-APACHE', 'LICENSE-MIT'],
  },
];

function prepared(destination, fingerprint) {
  const stamp = join(destination, '.backport');
  return (
    existsSync(stamp) &&
    existsSync(join(destination, 'Cargo.toml')) &&
    readFileSync(stamp, 'utf8') === fingerprint
  );
}

async function prepare({ name, version, sha256, licences }) {
  const release = `${name}-${version}`;
  const patch = join(PATCHES, `${release}.patch`);
  const licencePath = (licence) => join(PATCHES, `${name}-${licence}`);
  const hash = createHash('sha256').update(sha256).update(readFileSync(patch));
  for (const licence of licences) hash.update(readFileSync(licencePath(licence)));
  const fingerprint = hash.digest('hex');
  const destination = join(CACHE, name);
  if (prepared(destination, fingerprint)) return;

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
    capture('git', ['apply', '--whitespace=nowarn', `--directory=${fromRoot}`, patch]);
    for (const licence of licences) copyFileSync(licencePath(licence), join(source, licence));
    writeFileSync(join(source, '.backport'), fingerprint);

    // Finish all work before replacing an older generated copy.
    if (prepared(destination, fingerprint)) return;
    rmSync(destination, { recursive: true, force: true });
    renameSync(source, destination);
    console.log(`Applied the Rust backport for ${release}.`);
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
