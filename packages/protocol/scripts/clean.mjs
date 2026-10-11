// Empties src/generated before the types are written again, so a type removed from crates/protocol
// does not leave its file behind (ts-rs only ever writes files).
import { rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const generated = join(dirname(fileURLToPath(import.meta.url)), "..", "src", "generated");
rmSync(generated, { recursive: true, force: true });
