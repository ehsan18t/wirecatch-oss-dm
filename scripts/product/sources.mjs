// The Rust crate and the TypeScript module generated from config/product.toml, before formatting.
import { CHANNELS } from './read.mjs';

/** What every generated file says first, after any line its format requires there. */
export const GENERATED_NOTE =
  'Generated from config/product.toml by `pnpm product:generate`. Do not edit: change config/product.toml and run it again.';

/** Which languages use a constant: only those get it, so nothing is generated that nobody reads. */
const RUST = { rust: true, ts: false };
const TS = { rust: false, ts: true };
const BOTH = { rust: true, ts: true };

/** The product slug as the start of an environment variable's name: `wirecatch` gives `WIRECATCH`. */
const environmentName = (slug) => slug.toUpperCase().replaceAll('-', '_');

/** The constants generated: their name, what they mean, their value, and which languages use them. */
const CONSTANTS = [
  ['PRODUCT_NAME', 'The product name people see.', (p) => p.product.name, BOTH],
  ['PRODUCT_SLUG', 'The lowercase product name used in file names.', (p) => p.product.slug, TS],
  [
    'PRODUCT_VERSION',
    'The release version of the app and the extension.',
    (p) => p.product.version,
    RUST,
  ],
  ['WEBSITE_URL', 'The product website.', (p) => p.links.website, TS],
  ['APP_EXECUTABLE', 'The desktop app executable, without `.exe`.', (p) => p.executables.app, RUST],
  [
    'BRIDGE_EXECUTABLE',
    'The browser bridge executable, without `.exe`; it lives next to the app (D19).',
    (p) => p.executables.bridge,
    RUST,
  ],
  [
    'CLI_EXECUTABLE',
    'The command-line downloader executable, without `.exe`.',
    (p) => p.executables.cli,
    RUST,
  ],
  ['DATABASE_FILE', 'The download list, in the data folder.', (p) => p.files.database, RUST],
  [
    'LOG_FILTER_VARIABLE',
    'The environment variable that chooses which lines every binary logs, in `tracing` filter syntax (`debug`, `wirecatch_core=trace`).',
    (p) => `${environmentName(p.product.slug)}_LOG`,
    RUST,
  ],
  [
    'LOG_FOLDER_VARIABLE',
    "The environment variable that names another folder for every binary's log files.",
    (p) => `${environmentName(p.product.slug)}_LOG_DIR`,
    RUST,
  ],
  [
    'NATIVE_HOST_NAME',
    'The native messaging host name the bridge registers and the extension connects to.',
    (p) => p.native_host.name,
    BOTH,
  ],
  [
    'NATIVE_HOST_DESCRIPTION',
    "The description in the bridge's native messaging manifests.",
    (p) => p.native_host.description,
    RUST,
  ],
  [
    'CHROMIUM_EXTENSION_ID',
    'The one Chromium extension id every bridge trusts (D22).',
    (p) => p.extension.chromium_id,
    RUST,
  ],
  [
    'CHROMIUM_EXTENSION_ORIGIN',
    'The origin Chromium passes for the extension, in the exact form it uses.',
    (p) => `chrome-extension://${p.extension.chromium_id}/`,
    RUST,
  ],
  [
    'CHROMIUM_PUBLIC_KEY',
    'The public key that gives unpacked Chromium builds the extension id (D22).',
    (p) => p.extension.chromium_public_key,
    TS,
  ],
  ['FIREFOX_EXTENSION_ID', 'The Firefox add-on id.', (p) => p.extension.firefox_id, BOTH],
  [
    'EXTENSION_DESCRIPTION',
    "The extension's one line in browser stores.",
    (p) => p.extension.description,
    TS,
  ],
  [
    'IPC_RUNTIME_FOLDER',
    "Linux: the folder in `XDG_RUNTIME_DIR` that holds the app's socket.",
    (p) => p.ipc.runtime_folder,
    RUST,
  ],
  [
    'IPC_FALLBACK_FOLDER_PREFIX',
    'Linux: the start of the folder in `/tmp` that holds the socket when there is no runtime folder; the user id follows it.',
    (p) => p.ipc.fallback_folder_prefix,
    RUST,
  ],
];

/** The per-channel values, as `Channel` methods. */
const CHANNEL_METHODS = [
  ['name', 'The name this channel shows, which is also its start-at-sign-in entry.', 'name'],
  [
    'identifier',
    'The app identifier, which keeps the channels apart for the operating system and the single-instance guard.',
    'identifier',
  ],
  [
    'windows_data_folder',
    "The data folder's name in `%LOCALAPPDATA%` on Windows.",
    'windows_data_folder',
  ],
  [
    'linux_data_folder',
    "The data folder's name in `~/.local/share` on Linux.",
    'linux_data_folder',
  ],
  [
    'pipe_prefix',
    "Windows: the pipe name before the user's SID, after `\\\\.\\pipe\\`.",
    'pipe_prefix',
  ],
  ['socket_name', "Linux: the socket's file name in the runtime folder.", 'socket'],
];

const VARIANT = { release: 'Release', dev: 'Dev' };
const VARIANT_DOC = { release: 'Release builds.', dev: 'Debug builds.' };

/** A Rust string literal; read.mjs has made sure no value holds a control character. */
const rustString = (value) => `"${value.replaceAll(/[\\"]/g, String.raw`\$&`)}"`;

function rustConstants(product) {
  return CONSTANTS.filter(([, , , uses]) => uses.rust).map(
    ([name, doc, value]) =>
      `/// ${doc}\npub const ${name}: &str = ${rustString(value(product))};\n`,
  );
}

function rustChannel(product) {
  const methods = CHANNEL_METHODS.map(([method, doc, key]) => {
    const arms = CHANNELS.map(
      (channel) => `Self::${VARIANT[channel]} => ${rustString(product.channels[channel][key])},`,
    ).join('\n');
    return `/// ${doc}\n#[must_use]\npub const fn ${method}(self) -> &'static str {\nmatch self {\n${arms}\n}\n}\n`;
  });
  const variants = CHANNELS.map((c) => `/// ${VARIANT_DOC[c]}\n${VARIANT[c]},`).join('\n');
  return `/// A build of the app that runs as an app of its own, next to the others, without touching their downloads, endpoint or settings (D13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Channel {
${variants}
}

impl Channel {
${methods.join('\n')}
/// The data folder's name on this system: [\`Self::windows_data_folder\`] on Windows, [\`Self::linux_data_folder\`] elsewhere.
#[must_use]
pub const fn data_folder(self) -> &'static str {
if cfg!(windows) { self.windows_data_folder() } else { self.linux_data_folder() }
}
}
`;
}

/** The `wirecatch-product` crate's lib.rs, before rustfmt. */
export function rustSource(product) {
  const fileNames = [
    ['APP_FILE_NAME', 'APP_EXECUTABLE', product.executables.app],
    ['BRIDGE_FILE_NAME', 'BRIDGE_EXECUTABLE', product.executables.bridge],
  ].map(
    ([name, executable, value]) =>
      `/// [\`${executable}\`] as a file name on this system.\npub const ${name}: &str = if cfg!(windows) { ${rustString(`${value}.exe`)} } else { ${executable} };\n`,
  );
  return `// ${GENERATED_NOTE}

//! ${product.product.name}'s product identity: names, ids, executables, folders and addresses, written once in \`config/product.toml\` (D29).
//!
//! Every binary that needs one of these values takes it from here, so a rename changes one file. Values that are not decided yet hold a placeholder (see \`config/product.toml\`), which the release check refuses.

${rustConstants(product).join('\n')}
${fileNames.join('\n')}
${rustChannel(product)}`;
}

/** The `@wirecatch/product` package's index.ts, before Prettier. */
export function typescriptSource(product) {
  const constants = CONSTANTS.filter(([, , , uses]) => uses.ts).map(
    ([name, doc, value]) =>
      `/** ${doc} */\nexport const ${name} = ${JSON.stringify(value(product))};\n`,
  );
  return `// ${GENERATED_NOTE}\n\n${constants.join('\n')}`;
}
