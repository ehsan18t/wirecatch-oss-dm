// Writing product identity into files that also hold other things: fields of JSON and Cargo manifests, and templates whose `{{section.key}}` names a value of config/product.toml.

/** A file the generator cannot update as it is; the message says which and why. */
export class ManifestError extends Error {}

/** `text` (a JSON file) with `change` applied to its parsed value, ready for Prettier. */
export function editJson(text, change) {
  const json = JSON.parse(text);
  change(json);
  return `${JSON.stringify(json, null, 2)}\n`;
}

const TABLE_HEADER = /^\s*\[/;

/** The line numbers of the body of `header` (`[workspace.package]`, `[[bin]]`): from after it to the next table. Fails unless it appears exactly once. */
function tableBody(lines, header, path) {
  const starts = lines.flatMap((line, i) => (line.trim() === header ? [i] : []));
  if (starts.length !== 1) {
    throw new ManifestError(
      `${path} must have exactly one ${header} table for the generator to update; it has ${String(starts.length)}.`,
    );
  }
  const start = starts[0] + 1;
  const next = lines.findIndex((line, i) => i >= start && TABLE_HEADER.test(line));
  return { start, end: next === -1 ? lines.length : next };
}

/**
 * `text` (the Cargo.toml at `path`) with `key = value` set in the table `header`: the line that sets the key is replaced, or one is added after the table's last setting. `value` is already TOML (`"0.1.0"`, `["a"]`). Comments and every other line stay as they are.
 */
export function setTomlKey(text, path, header, [key, value]) {
  const lines = text.split('\n');
  const { start, end } = tableBody(lines, header, path);
  const setting = new RegExp(`^\\s*${key.replace(/\./g, '\\.')}\\s*=`);
  const line = `${key} = ${value}`;
  const existing = lines.findIndex((l, i) => i >= start && i < end && setting.test(l));
  if (existing !== -1) {
    lines[existing] = line;
  } else {
    let last = end - 1;
    while (last >= start && (lines[last].trim() === '' || lines[last].trim().startsWith('#')))
      last -= 1;
    lines.splice(last + 1, 0, line);
  }
  return lines.join('\n');
}

/** `text` (a Cargo.toml) with `version = "..."` set to `version` on every path dependency of the table `header`, the crates of this workspace. */
export function setPathDependencyVersions(text, header, version, path) {
  const lines = text.split('\n');
  const { start, end } = tableBody(lines, header, path);
  for (let i = start; i < end; i += 1) {
    if (/\bpath\s*=/.test(lines[i])) {
      lines[i] = lines[i].replace(/\bversion\s*=\s*"[^"]*"/, `version = ${tomlString(version)}`);
    }
  }
  return lines.join('\n');
}

/** A TOML basic string; read.mjs has made sure no value holds a control character. */
export const tomlString = (value) => `"${value.replace(/[\\"]/g, '\\$&')}"`;

/** The value of `product` at a dotted `path`, which must name a string. */
function valueAt(product, path, template) {
  let value = product;
  for (const key of path.split('.')) value = value?.[key];
  if (typeof value !== 'string') {
    throw new ManifestError(
      `${template} uses {{${path}}}, which is not a value in config/product.toml.`,
    );
  }
  return value;
}

/** `template` with every `{{section.key}}` replaced by that value of `product`. */
export function fillTemplate(template, product, name) {
  return template.replace(/\{\{\s*([a-z_.]+)\s*\}\}/g, (_, path) => valueAt(product, path, name));
}

/** How each kind of file writes a comment line, and whether a first line (a `#!`) must stay first. */
const COMMENT_STYLES = {
  '.sh': { line: (text) => `# ${text}`, keepFirstLine: true },
  '.nsh': { line: (text) => `; ${text}`, keepFirstLine: false },
  '.md': { line: (text) => `<!-- ${text} -->`, keepFirstLine: false },
};

/** `text` with the comment `note` at the top, after a `#!` line where the format needs one first. */
export function withNote(text, note, path) {
  const extension = Object.keys(COMMENT_STYLES).find((ext) => path.endsWith(ext));
  if (!extension)
    throw new ManifestError(`No comment style for ${path}; add one to COMMENT_STYLES.`);
  const style = COMMENT_STYLES[extension];
  const comment = `${style.line(note)}\n`;
  if (style.keepFirstLine && text.startsWith('#!')) {
    const firstLineEnd = text.indexOf('\n') + 1;
    return text.slice(0, firstLineEnd) + comment + text.slice(firstLineEnd);
  }
  return comment + text;
}
