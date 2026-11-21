// Writing product identity into files that also hold other things: fields of JSON and Cargo manifests, and templates whose `{{section.key}}` names a value of config/product.toml.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

/** A file the generator cannot update as it is; the message says which and why. */
export class ManifestError extends Error {}

/** The crates of the workspace: the `members` of the root Cargo.toml, each a folder or a `folder/*` glob. */
export function crateManifests(root) {
  const members = /^members\s*=\s*\[([^\]]*)\]/m.exec(
    readFileSync(join(root, 'Cargo.toml'), 'utf8'),
  );
  if (!members) throw new ManifestError('The root Cargo.toml has no `members` list.');
  return [...members[1].matchAll(/"([^"]+)"/g)].flatMap(([, member]) => {
    const folders = member.endsWith('/*')
      ? readdirSync(join(root, member.slice(0, -2)), { withFileTypes: true })
          .filter((entry) => entry.isDirectory())
          .map((entry) => `${member.slice(0, -2)}/${entry.name}`)
      : [member];
    return folders
      .map((folder) => `${folder}/Cargo.toml`)
      .filter((path) => existsSync(join(root, path)));
  });
}

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

/** Package names of the workspace members that carry the product version. */
export function workspaceCrateNames(root) {
  return new Set(
    crateManifests(root).map((path) => {
      const lines = readFileSync(join(root, path), 'utf8').split('\n');
      const { start, end } = tableBody(lines, '[package]', path);
      const name = /^name\s*=\s*"([^"]+)"/m.exec(lines.slice(start, end).join('\n'))?.[1];
      if (!name) throw new ManifestError(`${path} has no package name.`);
      return name;
    }),
  );
}

/** Whether a TOML value goes on past its line: an unclosed array or table, or a multi-line string. */
function continuesOnNextLine(value) {
  const opened = (value.match(/[[{]/g) ?? []).length;
  const closed = (value.match(/[\]}]/g) ?? []).length;
  return opened > closed || value.startsWith('"""') || value.startsWith("'''");
}

/**
 * `text` (the Cargo.toml at `path`) with `key = value` set in the table `header`: the line that sets the key is replaced, or one is added after the table's last setting. `value` is already TOML (`"0.1.0"`, `["a"]`). Comments and every other line stay as they are.
 */
export function setTomlKey(text, path, header, [key, value]) {
  const lines = text.split('\n');
  const { start, end } = tableBody(lines, header, path);
  // The indentation, the key, its value and a trailing comment that holds no quote.
  const setting = new RegExp(
    String.raw`^(\s*)${key.replaceAll('.', String.raw`\.`)}\s*=\s*(.*?)(\s+#[^"']*)?$`,
  );
  const line = `${key} = ${value}`;
  const existing = lines.findIndex((l, i) => i >= start && i < end && setting.test(l));
  if (existing === -1) {
    let last = end - 1;
    while (last >= start && (lines[last].trim() === '' || lines[last].trim().startsWith('#')))
      last -= 1;
    lines.splice(last + 1, 0, line);
  } else {
    const [, indent, current, comment = ''] = setting.exec(lines[existing]) ?? [];
    if (continuesOnNextLine(current ?? '')) {
      throw new ManifestError(
        `${path}: ${key} in ${header} takes more than one line; write it on one line for the generator to update it.`,
      );
    }
    lines[existing] = `${indent ?? ''}${line}${comment}`;
  }
  return lines.join('\n');
}

/** `text` (a Cargo.toml) with `version = "..."` set to `version` on every path dependency of the table `header`, the crates of this workspace. */
export function setPathDependencyVersions(text, header, version, path) {
  const lines = text.split('\n');
  const { start, end } = tableBody(lines, header, path);
  for (let i = start; i < end; i += 1) {
    if (/\bpath\s*=/.test(lines[i])) {
      lines[i] = lines[i].replace(
        /\bversion\s*=\s*"[^"]*"/,
        () => `version = ${tomlString(version)}`,
      );
    }
  }
  return lines.join('\n');
}

/** A TOML basic string; read.mjs has made sure no value holds a control character. */
export const tomlString = (value) => `"${value.replaceAll(/[\\"]/g, String.raw`\$&`)}"`;

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
  const placeholder = /\{\{\s*([a-z_.]+)\s*\}\}/g;
  // Checked in the template, not the result, so a value holding braces is not taken for one.
  const leftover = /\{\{[^\n]*/.exec(template.replaceAll(placeholder, ''));
  if (leftover) {
    throw new ManifestError(
      `${name} has a placeholder that is not of the form {{section.key}}: ${leftover[0]}`,
    );
  }
  return template.replaceAll(placeholder, (_, path) => valueAt(product, path, name));
}
