// `icon`: the app's icon draws the same mark as the interface (D108.3). The installer, tray and taskbar pictures are rendered from the SVG, and an SVG cannot read a JSON file, so the strokes are compared here.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { GateError, ROOT } from './process.mjs';

const ICON = 'crates/desktop/assets/icons/icon.svg';
const MARK = 'packages/ui/src/mark.json';

/** The strokes the polylines of `svg` draw, shaped as in mark.json. */
function iconStrokes(svg) {
  return [...svg.matchAll(/<polyline\b[^>]*>/g)].map(([polyline]) => ({
    points: (/\bpoints="([^"]*)"/.exec(polyline)?.[1] ?? '')
      .trim()
      .split(/\s+/)
      .map((point) => point.split(',').map(Number)),
    width: Number(/\bstroke-width="([^"]*)"/.exec(polyline)?.[1]),
  }));
}

/** What differs between the icon `svg` and the `mark`, or null when they draw the same strokes. */
export function iconProblem(svg, mark) {
  const drawn = JSON.stringify(iconStrokes(svg));
  const expected = JSON.stringify(mark.strokes);
  return drawn === expected ? null : `${ICON} draws ${drawn}, but ${MARK} says ${expected}.`;
}

function iconMatchesMark() {
  const problem = iconProblem(
    readFileSync(join(ROOT, ICON), 'utf8'),
    JSON.parse(readFileSync(join(ROOT, MARK), 'utf8')),
  );
  if (problem !== null) throw new GateError(problem);
}

export const ICON_CHECKS = { icon: iconMatchesMark };
