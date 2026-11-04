// Reads pnpm-workspace.yaml without a YAML package: the gate runs before any package is installed (scripts/linux-test.sh).

/** The folders of the `packages` globs in `manifest` (each a `folder/*`). A list it cannot read throws a `Failure`. */
export function workspaceParents(manifest, Failure) {
  const list = /^packages:[ \t]*\r?\n((?:(?:[ \t]*\r?\n)*[ \t]+-[ \t]+(?:\S.*)?\r?\n)+)/m.exec(
    manifest,
  );
  if (!list) throw new Failure('pnpm-workspace.yaml has no `packages` list.');
  const globs = [...list[1].matchAll(/-\s+["']?([^"'\s]+)["']?/g)].map((m) => m[1]);
  return globs.map((glob) => {
    if (!glob.endsWith('/*')) {
      throw new Failure(
        `The workspace glob ${glob} is not of the form folder/*; teach workspaceParents in scripts/workspace.mjs to read it.`,
      );
    }
    return glob.slice(0, -2);
  });
}
