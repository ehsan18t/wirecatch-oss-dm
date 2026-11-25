// Reads pnpm-workspace.yaml without a YAML package: the gate runs before any package is installed (scripts/linux-test.sh).
import { readdirSync } from 'node:fs';
import { join } from 'node:path';

/** The `packages` globs in `manifest`, each a folder of packages (`folder/*`, `nested`) or one package (`folder`). A list it cannot read throws a `Failure`. */
export function workspaceGlobs(manifest, Failure) {
  const list = /^packages:[ \t]*\r?\n((?:(?:[ \t]*\r?\n)*[ \t]+-[ \t]+(?:\S.*)?\r?\n)+)/m.exec(
    manifest,
  );
  if (!list) throw new Failure('pnpm-workspace.yaml has no `packages` list.');
  const globs = [...list[1].matchAll(/-\s+["']?([^"'\s]+)["']?/g)].map((m) => m[1]);
  return globs.map((glob) => {
    const nested = glob.endsWith('/*');
    const folder = nested ? glob.slice(0, -2) : glob;
    if (!/^[\w.-]+$/.test(folder)) {
      throw new Failure(
        `The workspace glob ${glob} is neither folder/* nor a folder; teach workspaceGlobs in scripts/workspace.mjs to read it.`,
      );
    }
    return { folder, nested };
  });
}

/** The folders under `root` the globs can make packages of: every folder in a nested one, and each single one. */
export function candidateFolders(root, globs) {
  return globs.flatMap(({ folder, nested }) =>
    nested
      ? readdirSync(join(root, folder), { withFileTypes: true })
          .filter((entry) => entry.isDirectory())
          .map((entry) => `${folder}/${entry.name}`)
      : [folder],
  );
}
