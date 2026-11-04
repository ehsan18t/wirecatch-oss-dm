// The third-party notices of every installer can be generated from the lockfile (D117).
import { installerPackages } from '../notices/cargo.mjs';
import { shippedTargets } from '../notices/licences.mjs';
import { renderNotices } from '../notices/render.mjs';

function notices() {
  for (const triple of shippedTargets()) {
    const packages = installerPackages(triple);
    const { length } = renderNotices({ title: triple, packages });
    console.log(`${triple}: ${String(packages.length)} crates, ${String(length)} characters.`);
  }
}

export const NOTICES_CHECKS = { notices };
