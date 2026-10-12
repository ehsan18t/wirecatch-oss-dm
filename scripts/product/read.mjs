// Reads config/product.toml and checks every value once, so nothing generated from it has to: a value of the wrong shape stops the generator and the gate with a message naming it.
import { createHash } from 'node:crypto';
import { parse } from 'smol-toml';

/** Every value a placeholder contains (config/product.toml explains the convention). */
export const PLACEHOLDER_MARK = 'TODO-PRODUCT-';

/** A product.toml that cannot be used; the message lists every problem found. */
export class ProductError extends Error {}

/** Chromium derives an extension id from the first 16 bytes of the SHA-256 of its public key. */
const CHROMIUM_ID_BYTES = 16;
/** Chromium writes each hexadecimal digit of those bytes as a letter from `a` to `p`. */
const HEX = 16;
const FIRST_ID_LETTER = 'a'.charCodeAt(0);

const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;
const SLUG = /^[a-z][a-z0-9-]*$/;
/** Reverse-DNS identifiers, as Tauri and the operating systems accept them. */
const IDENTIFIER = /^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)+$/;
/** Chromium's rule for host names: lowercase letters, digits, `_` and `.`, no dot at either end and never two in a row. */
const HOST_NAME = /^[a-z0-9_]+(\.[a-z0-9_]+)*$/;
const CHROMIUM_ID = /^[a-p]{32}$/;
/** Firefox add-on ids are email-like or a braced GUID. */
const FIREFOX_ID = /^([A-Za-z0-9._-]+@[A-Za-z0-9.-]+|\{[0-9a-fA-F-]{36}\})$/;
const EMAIL = /^[^@\s]+@[^@\s]+\.[^@\s]+$/;
/** One folder or file name: no separator, nothing a Windows or Linux file name cannot hold. */
const FILE_NAME = /^(?!\.{1,2}$)[^\\/:*?"<>|\p{Cc}]+$/u;
const PIPE_PREFIX = /^[A-Za-z0-9-]+-$/;
const SOCKET = /^[a-z0-9-]+\.sock$/;
/** Printable text on one line. */
const TEXT = /^(?!\s*$)\P{Cc}+$/u;

/** Each section and key product.toml must have, with the shape its value must take. Unknown keys are an error, so a new value is never ignored. */
const SCHEMA = {
  product: {
    name: TEXT,
    slug: SLUG,
    version: SEMVER,
    license: /^[A-Za-z0-9.+-]+( (AND|OR|WITH) [A-Za-z0-9.+-]+)*$/,
    authors: TEXT,
    summary: TEXT,
    description: TEXT,
  },
  publisher: { legal_name: TEXT },
  links: { repository: 'url', website: 'url' },
  contacts: { support_email: EMAIL, security_email: EMAIL },
  executables: { app: SLUG, bridge: SLUG, cli: SLUG },
  files: { database: FILE_NAME },
  native_host: { name: HOST_NAME, description: TEXT },
  extension: {
    chromium_id: CHROMIUM_ID,
    chromium_public_key: /^[A-Za-z0-9+/]+={0,2}$/,
    firefox_id: FIREFOX_ID,
    description: TEXT,
  },
  ipc: { runtime_folder: FILE_NAME, fallback_folder_prefix: FILE_NAME },
};

/** What every channel has; the release channel shows the product name, the others a name of their own. */
const CHANNEL = {
  identifier: IDENTIFIER,
  windows_data_folder: FILE_NAME,
  linux_data_folder: FILE_NAME,
  pipe_prefix: PIPE_PREFIX,
  socket: SOCKET,
};
export const CHANNELS = ['release', 'dev'];
/** Values two channels may never share: they would then use one app's data, endpoint or identity. */
const DISTINCT_PER_CHANNEL = ['name', ...Object.keys(CHANNEL)];

function isHttpsUrl(value) {
  try {
    return new URL(value).protocol === 'https:';
  } catch {
    return false;
  }
}

/** The problems with one table's values, each naming its key. */
function tableProblems(where, table, shape) {
  if (typeof table !== 'object' || table === null || Array.isArray(table)) {
    return [`[${where}] is missing.`];
  }
  const problems = Object.keys(table)
    .filter((key) => !(key in shape))
    .map(
      (key) =>
        `${where}.${key} is not a known value; add it to SCHEMA in scripts/product/read.mjs first.`,
    );
  for (const [key, rule] of Object.entries(shape)) {
    const value = table[key];
    if (typeof value !== 'string') problems.push(`${where}.${key} must be a string.`);
    else if (rule === 'url' ? !isHttpsUrl(value) : !rule.test(value)) {
      problems.push(`${where}.${key} = ${JSON.stringify(value)} does not have the expected form.`);
    }
  }
  return problems;
}

/** The Chromium extension id that follows from a base64 public key. */
export function chromiumIdOf(publicKey) {
  const digest = createHash('sha256').update(Buffer.from(publicKey, 'base64')).digest('hex');
  return [...digest.slice(0, CHROMIUM_ID_BYTES * 2)]
    .map((digit) => String.fromCharCode(FIRST_ID_LETTER + Number.parseInt(digit, HEX)))
    .join('');
}

function channelProblems(raw) {
  const channels = raw.channels ?? {};
  const problems = Object.keys(channels)
    .filter((name) => !CHANNELS.includes(name))
    .map((name) => `channels.${name} is not a known channel (${CHANNELS.join(', ')}).`);
  for (const name of CHANNELS) {
    const own = name === 'release' ? CHANNEL : { name: TEXT, ...CHANNEL };
    problems.push(...tableProblems(`channels.${name}`, channels[name], own));
  }
  return problems;
}

/** The checked values, with each channel's name filled in. */
function normalized(raw) {
  const channels = Object.fromEntries(
    CHANNELS.map((name) => [name, { name: raw.product.name, ...raw.channels[name] }]),
  );
  return { ...raw, channels };
}

function crossProblems(product) {
  const problems = [];
  const { chromium_id: id, chromium_public_key: key } = product.extension;
  if (chromiumIdOf(key) !== id) {
    problems.push(
      `extension.chromium_id ${id} does not follow from extension.chromium_public_key (that key gives ${chromiumIdOf(key)}). Change both together (D22).`,
    );
  }
  for (const key of DISTINCT_PER_CHANNEL) {
    const values = CHANNELS.map((name) => product.channels[name][key]);
    if (new Set(values).size !== values.length) {
      problems.push(`Every channel needs its own ${key}; ${values.join(', ')} repeat one.`);
    }
  }
  return problems;
}

/** The product identity in `text` (the contents of config/product.toml), checked. Throws a ProductError listing every problem. */
export function readProduct(text) {
  let raw;
  try {
    raw = parse(text);
  } catch (error) {
    throw new ProductError(`config/product.toml is not valid TOML: ${error.message}`);
  }
  const problems = Object.keys(raw)
    .filter((key) => !(key in SCHEMA) && key !== 'channels')
    .map((key) => `[${key}] is not a known section.`);
  for (const [section, shape] of Object.entries(SCHEMA)) {
    problems.push(...tableProblems(section, raw[section], shape));
  }
  problems.push(...channelProblems(raw));
  if (problems.length > 0) throw new ProductError(problems.join('\n'));
  const product = normalized(raw);
  const cross = crossProblems(product);
  if (cross.length > 0) throw new ProductError(cross.join('\n'));
  return product;
}

/** Every value of `product` that is still a placeholder, as `section.key`. */
export function placeholders(product) {
  const found = [];
  const walk = (value, path) => {
    if (typeof value === 'string') {
      if (value.includes(PLACEHOLDER_MARK)) found.push(path.join('.'));
    } else {
      for (const [key, inner] of Object.entries(value)) walk(inner, [...path, key]);
    }
  };
  walk(product, []);
  return found;
}
