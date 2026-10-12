// The configuration package lints itself with its own baseline. Its rules are plain JavaScript, so they are linted without type information, like every package's scripts.
import { globals, wirecatch } from './index.js';

export default wirecatch({
  tsconfigRootDir: import.meta.dirname,
  globals: globals.node,
  ignores: ['rules/fixtures/'],
  untyped: ['**/*.js'],
});
