#!/usr/bin/env node
// Builds the GPUI desktop app's installers (D106): on Windows the Inno Setup installer, on Linux the .deb, the .rpm and the AppImage. They land in target/packages, named <product>-<version>-<system>-<architecture>.
//
// Usage: node scripts/package.mjs [--target <rust target triple>]. The default target is this machine's.
//
// The app and the bridge are built in release with the product identity checked first (D29, D49). The Linux tools (nfpm, appimagetool and the AppImage runtime) are downloaded at pinned versions and checked against their SHA-256 before use. Inno Setup is installed beforehand (CI installs the pinned version; locally, Inno Setup 6.7 or later).

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  chmodSync,
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';
import { ROOT } from './gate/process.mjs';
import { fillTemplate } from './product/manifests.mjs';
import { readProduct } from './product/read.mjs';

const PACKAGING = join(ROOT, 'crates', 'desktop', 'packaging');
const ICONS = join(ROOT, 'crates', 'desktop', 'assets', 'icons');
const OUT = join(ROOT, 'target', 'packages');
const TOOLS = join(ROOT, 'target', 'tools');

/** Downloads by architecture, with the SHA-256 each must have. */
const LINUX_TOOLS = {
  nfpm: {
    version: '2.48.0',
    x64: [
      'https://github.com/goreleaser/nfpm/releases/download/v2.48.0/nfpm_2.48.0_Linux_x86_64.tar.gz',
      '6d7f9a28b2f397934403950fcbba83f598b08d02edbe79a0815b3b4897341750',
    ],
    arm64: [
      'https://github.com/goreleaser/nfpm/releases/download/v2.48.0/nfpm_2.48.0_Linux_arm64.tar.gz',
      'a9a75213763783b7669719e505474c748c5ad7dc66591a195619deb50f85c2f4',
    ],
  },
  appimagetool: {
    version: '1.9.1',
    x64: [
      'https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage',
      'ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0',
    ],
    arm64: [
      'https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-aarch64.AppImage',
      'f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158',
    ],
  },
  // appimagetool fetches the newest runtime unless it is given one.
  runtime: {
    version: '20251108',
    x64: [
      'https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64',
      '2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d',
    ],
    arm64: [
      'https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-aarch64',
      '00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444',
    ],
  },
};

/** What the app needs from the system on Linux: the libraries GPUI loads, by package name. */
const LINUX_DEPENDS = {
  deb: [
    'libfontconfig1',
    'libfreetype6',
    'libxkbcommon0',
    'libxkbcommon-x11-0',
    'libwayland-client0',
    'libx11-xcb1',
    'libxcb1',
    'libvulkan1',
  ],
  rpm: [
    'fontconfig',
    'freetype',
    'libxkbcommon',
    'libxkbcommon-x11',
    'libwayland-client',
    'libX11-xcb',
    'libxcb',
    'vulkan-loader',
  ],
};

const ICON_SIZES = [32, 64, 128, 256, 512];

/**
 * The libraries an AppImage carries: those the app links that a desktop without GTK or Qt may lack.
 * The rest (the C library, X11, fontconfig, freetype, Wayland, Vulkan) come with every desktop
 * and must be the system's own, as AppImage's exclude list says.
 */
const APPIMAGE_LIBRARIES = ['libxkbcommon.so.0', 'libxkbcommon-x11.so.0'];

/** Copies the `APPIMAGE_LIBRARIES` the app at `binary` links, as this machine resolves them, into `folder`. */
function bundleLibraries(binary, folder) {
  mkdirSync(folder, { recursive: true });
  // Lines read `name => /path/name (0x...)`.
  const linked = new Map(
    execFileSync('ldd', [binary], { encoding: 'utf8' })
      .split('\n')
      .map((line) => line.trim().split(/\s+/))
      .filter((parts) => parts[1] === '=>')
      .map(([name, , path]) => [name, path]),
  );
  for (const name of APPIMAGE_LIBRARIES) {
    const path = linked.get(name);
    if (!path?.startsWith('/')) fail(`The app does not link ${name}, or this machine lacks it.`);
    copyFileSync(path, join(folder, name));
  }
}

/** The AppImage's entry point: the app, finding the libraries the AppImage carries first. */
function appRun(app) {
  return `#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
export LD_LIBRARY_PATH="$HERE/usr/lib\${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$HERE/usr/bin/${app}" "$@"
`;
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

function run(command, args, options = {}) {
  execFileSync(command, args, { cwd: ROOT, stdio: 'inherit', ...options });
}

function targetTriple() {
  const at = process.argv.indexOf('--target');
  if (at !== -1) return process.argv[at + 1] ?? fail('--target needs a target triple.');
  const host = /^host: (.+)$/m.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))?.[1];
  return host ?? fail('Could not read the target triple from `rustc -vV`. Is Rust installed?');
}

/** The architecture as release files name it. */
function architecture(triple) {
  if (triple.startsWith('x86_64-')) return 'x64';
  if (triple.startsWith('aarch64-')) return 'arm64';
  return fail(`No installer is made for ${triple}.`);
}

function build(triple) {
  try {
    run(process.execPath, [join(ROOT, 'scripts', 'gate.mjs'), 'release-build']);
  } catch {
    fail('The product identity is out of date; see the message above.');
  }
  const env = { ...process.env };
  // The Visual C++ runtime is linked in, so the app runs on a Windows without it installed (D33).
  if (triple.endsWith('-windows-msvc')) {
    env.RUSTFLAGS = `${env.RUSTFLAGS ?? ''} -C target-feature=+crt-static`.trim();
  }
  // No cargo feature: the bridge's one feature, test-overrides, is for tests only (D22).
  const crates = ['-p', 'wirecatch-desktop', '-p', 'wirecatch-bridge'];
  run('cargo', ['build', '--release', '--locked', ...crates, '--target', triple], { env });
  const dir = process.env.CARGO_TARGET_DIR ?? join(ROOT, 'target');
  return join(dir, triple, 'release');
}

/** The app and the bridge side by side, where they find each other by name (D19). */
function stageBinaries(product, built, into, exe) {
  mkdirSync(into, { recursive: true });
  for (const name of [product.executables.app, product.executables.bridge]) {
    copyFileSync(join(built, `${name}${exe}`), join(into, `${name}${exe}`));
  }
}

function windows(product, built, arch) {
  const stage = join(OUT, `stage-windows-${arch}`);
  rmSync(stage, { recursive: true, force: true });
  stageBinaries(product, built, stage, '.exe');
  copyFileSync(join(ROOT, 'LICENSE'), join(stage, 'LICENSE.txt'));
  const iscc =
    process.env.ISCC ??
    join(
      process.env['ProgramFiles(x86)'] ?? String.raw`C:\Program Files (x86)`,
      'Inno Setup 6',
      'ISCC.exe',
    );
  if (!existsSync(iscc))
    fail(`Inno Setup's compiler is not at ${iscc}. Install Inno Setup 6.7 or later, or set ISCC.`);
  const { version } = product.product;
  const defines = {
    AppId: product.channels.release.identifier,
    AppName: product.product.name,
    AppVersion: version,
    FileVersion: `${version.split(/[-+]/)[0]}.0`,
    Publisher: product.publisher.legal_name,
    Website: product.links.website,
    Repository: product.links.repository,
    AppExe: product.executables.app,
    BridgeExe: product.executables.bridge,
    Arch: arch === 'x64' ? 'x64compatible' : 'arm64',
    Icon: join(ICONS, 'icon.ico'),
    Stage: stage,
    Output: OUT,
    OutputName: `${product.product.name}-${version}-windows-${arch}-setup`,
  };
  run(iscc, [
    '/Q',
    ...Object.entries(defines).map(([key, value]) => `/D${key}=${value}`),
    join(PACKAGING, 'windows', 'installer.iss'),
  ]);
}

/** A pinned download in target/tools, checked against its SHA-256. */
async function tool(name, arch) {
  const {
    version,
    [arch]: [url, sha256],
  } = LINUX_TOOLS[name];
  const file = join(TOOLS, `${name}-${version}-${arch}`, url.split('/').at(-1));
  let bytes;
  if (existsSync(file)) {
    bytes = readFileSync(file);
  } else {
    const response = await fetch(url);
    if (!response.ok) fail(`Could not download ${url}: ${response.status}`);
    bytes = Buffer.from(await response.arrayBuffer());
  }
  // Checked on every run, so a file changed in target/tools is never used.
  const got = createHash('sha256').update(bytes).digest('hex');
  if (got !== sha256) fail(`${url} has SHA-256 ${got}, not the pinned ${sha256}.`);
  mkdirSync(join(file, '..'), { recursive: true });
  writeFileSync(file, bytes, { mode: 0o755 });
  return file;
}

function desktopEntry(product) {
  const id = product.channels.release.identifier;
  return [
    '[Desktop Entry]',
    'Type=Application',
    `Name=${product.product.name}`,
    `Comment=${product.product.summary}`,
    `Exec=${product.executables.app} %U`,
    `Icon=${id}`,
    'Terminal=false',
    'Categories=Network;FileTransfer;',
    `StartupWMClass=${id}`,
    '',
  ].join('\n');
}

/** The files every Linux package installs, under `root` as they go under `/`. */
function stageLinux(product, built, root) {
  const id = product.channels.release.identifier;
  rmSync(root, { recursive: true, force: true });
  stageBinaries(product, built, join(root, 'usr', 'bin'), '');
  const applications = join(root, 'usr', 'share', 'applications');
  mkdirSync(applications, { recursive: true });
  writeFileSync(join(applications, `${id}.desktop`), desktopEntry(product));
  for (const size of ICON_SIZES) {
    const folder = join(root, 'usr', 'share', 'icons', 'hicolor', `${size}x${size}`, 'apps');
    mkdirSync(folder, { recursive: true });
    const source =
      size === 512 ? 'icon.png' : size === 256 ? '128x128@2x.png' : `${size}x${size}.png`;
    copyFileSync(join(ICONS, source), join(folder, `${id}.png`));
  }
  const license = join(root, 'usr', 'share', 'licenses', product.product.slug);
  mkdirSync(license, { recursive: true });
  copyFileSync(join(ROOT, 'LICENSE'), join(license, 'LICENSE'));
}

async function linux(product, built, arch) {
  const { name, slug, version, summary, description, license } = product.product;
  const root = join(OUT, `stage-linux-${arch}`);
  stageLinux(product, built, root);
  const prerm = join(OUT, `prerm-${arch}.sh`);
  const template = readFileSync(join(ROOT, 'config', 'templates', 'prerm.sh'), 'utf8');
  writeFileSync(prerm, fillTemplate(template, product, 'prerm.sh'), { mode: 0o755 });

  const nfpmArchive = await tool('nfpm', arch);
  const nfpmDir = join(nfpmArchive, '..');
  run('tar', ['-xzf', nfpmArchive, '-C', nfpmDir, 'nfpm']);
  // JSON is YAML, which nfpm reads.
  const config = {
    name: slug,
    arch: arch === 'x64' ? 'amd64' : 'arm64',
    platform: 'linux',
    version,
    section: 'net',
    priority: 'optional',
    maintainer: `${product.publisher.legal_name} <${product.contacts.support_email}>`,
    description: `${summary}\n${description}`,
    vendor: product.publisher.legal_name,
    homepage: product.links.website,
    license,
    contents: [
      ...[product.executables.app, product.executables.bridge].map((file) => ({
        src: join(root, 'usr', 'bin', file),
        dst: `/usr/bin/${file}`,
        file_info: { mode: 0o755 },
      })),
      { src: `${join(root, 'usr', 'share')}/`, dst: '/usr/share', type: 'tree' },
    ],
    scripts: { preremove: prerm },
    overrides: { deb: { depends: LINUX_DEPENDS.deb }, rpm: { depends: LINUX_DEPENDS.rpm } },
  };
  const configFile = join(OUT, `nfpm-${arch}.yaml`);
  writeFileSync(configFile, JSON.stringify(config, null, 2));
  for (const format of ['deb', 'rpm']) {
    const target = join(OUT, `${name}-${version}-linux-${arch}.${format}`);
    run(join(nfpmDir, 'nfpm'), [
      'package',
      '--config',
      configFile,
      '--packager',
      format,
      '--target',
      target,
    ]);
  }

  const appDir = join(OUT, `${name}.AppDir`);
  rmSync(appDir, { recursive: true, force: true });
  cpSync(root, appDir, { recursive: true });
  const id = product.channels.release.identifier;
  copyFileSync(
    join(root, 'usr', 'share', 'applications', `${id}.desktop`),
    join(appDir, `${id}.desktop`),
  );
  copyFileSync(join(ICONS, '128x128@2x.png'), join(appDir, `${id}.png`));
  symlinkSync(`${id}.png`, join(appDir, '.DirIcon'));
  bundleLibraries(join(appDir, 'usr', 'bin', product.executables.app), join(appDir, 'usr', 'lib'));
  writeFileSync(join(appDir, 'AppRun'), appRun(product.executables.app), { mode: 0o755 });
  const appimagetool = await tool('appimagetool', arch);
  const runtime = await tool('runtime', arch);
  chmodSync(appimagetool, 0o755);
  run(
    appimagetool,
    [
      '--no-appstream',
      '--runtime-file',
      runtime,
      appDir,
      join(OUT, `${name}-${version}-linux-${arch}.AppImage`),
    ],
    {
      // No FUSE in containers and CI runners: the tool unpacks itself instead of mounting.
      env: {
        ...process.env,
        ARCH: arch === 'x64' ? 'x86_64' : 'aarch64',
        APPIMAGE_EXTRACT_AND_RUN: '1',
      },
    },
  );
}

const product = readProduct(readFileSync(join(ROOT, 'config', 'product.toml'), 'utf8'));
const triple = targetTriple();
const arch = architecture(triple);
mkdirSync(OUT, { recursive: true });
const built = build(triple);
if (triple.includes('-windows-')) windows(product, built, arch);
else if (triple.includes('-linux-')) await linux(product, built, arch);
else fail(`No installer is made for ${triple}.`);
console.log(`Installers are in ${OUT}.`);
