import { globals, wirecatch } from '@wirecatch/eslint-config';

export default wirecatch({
  tsconfigRootDir: import.meta.dirname,
  globals: globals.node,
  // ts-rs writes these from crates/protocol in its own style (`Array<T>`, object types); the Rust source is what is reviewed and linted, and `check` type-checks the output.
  // The generator's working folders next to it are ignored too.
  ignores: ['src/generated/', 'src/generated.*/'],
});
