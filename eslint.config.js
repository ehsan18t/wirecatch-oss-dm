// The repository's own scripts (`scripts/`), linted with the shared configuration like every package. Each package has its own eslint.config.js, which ESLint finds first for the files in it.
import { globals, wirecatch } from '@wirecatch/eslint-config';

export default wirecatch({
  tsconfigRootDir: import.meta.dirname,
  globals: globals.node,
  ignores: ['apps/', 'packages/', 'crates/', 'target/', 'config/', 'docs/'],
});
