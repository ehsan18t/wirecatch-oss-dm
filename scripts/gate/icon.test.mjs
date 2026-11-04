import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { iconProblem } from './icon.mjs';
import { ROOT } from './process.mjs';

const MARK = {
  strokes: [
    {
      points: [
        [1, 2],
        [3, 4],
      ],
      width: 5,
    },
  ],
};
const icon = (points, width) =>
  `<svg><polyline points="${points}" stroke="#fff" stroke-width="${String(width)}"/></svg>`;

test("the repository's icon draws the mark", () => {
  assert.equal(
    iconProblem(
      readFileSync(join(ROOT, 'crates/desktop/assets/icons/icon.svg'), 'utf8'),
      JSON.parse(readFileSync(join(ROOT, 'packages/ui/src/mark.json'), 'utf8')),
    ),
    null,
  );
});

test('an icon drawing other strokes is reported', () => {
  assert.equal(iconProblem(icon('1,2 3,4', 5), MARK), null);
  assert.match(iconProblem(icon('1,2 3,9', 5), MARK), /icon\.svg draws/);
  assert.match(iconProblem(icon('1,2 3,4', 6), MARK), /icon\.svg draws/);
  assert.match(iconProblem('<svg></svg>', MARK), /icon\.svg draws/);
});
