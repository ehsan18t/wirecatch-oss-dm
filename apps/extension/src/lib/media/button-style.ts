/** How the media button looks: its styles, kept inside its closed shadow root, and its icon. */

/** The button's height, set by `.b` in `SHADOW_CSS`. */
export const BUTTON_HEIGHT = 32;
/** About the button's width with its label, used to place it before it can be measured. */
export const TYPICAL_BUTTON_WIDTH = 112;

/**
 * The host's own styles. Important rules in a shadow root's `:host` beat the page's
 * important rules (the inner context wins), so a page rule such as
 * `* { display: none !important }` cannot hide or move the button. The position is
 * set inline, since `:host` would also win over the inline style; hiding the
 * button detaches the host instead of styling it.
 */
const HOST_RULES = [
  'position:fixed',
  'z-index:2147483647',
  'display:block',
  'visibility:visible',
  'opacity:1',
  // The host's box never takes the pointer; only the visible button does (see `.b`),
  // so the faded-out button cannot swallow clicks meant for the player's controls.
  'pointer-events:none',
  'margin:0',
  'padding:0',
  'border:0',
  'transform:none',
  'filter:none',
  'clip-path:none',
  'width:auto',
  'height:auto',
  'max-width:none',
  'max-height:none',
  'right:auto',
  'bottom:auto',
  'contain:none',
  'zoom:1',
]
  .map((rule) => `${rule}!important`)
  .join(';');

const { surface, ink, accent, danger } = __MEDIA_BUTTON_COLORS__;

/** The button's look is the app's dark scheme over any page: graphite and its text, the cyan accent under the pointer and round keyboard focus (D36). */
export const SHADOW_CSS = `
:host{${HOST_RULES}}
.w{all:initial;position:relative;display:block;pointer-events:none}
.b{all:initial;box-sizing:border-box;display:inline-flex;align-items:center;gap:6px;height:${String(BUTTON_HEIGHT)}px;padding:0 12px 0 10px;border-radius:16px;background:${surface}e6;color:${ink};font:600 13px/1 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;letter-spacing:.01em;white-space:nowrap;cursor:pointer;box-shadow:0 2px 10px rgba(0,0,0,.35),inset 0 0 0 1px ${ink}33;transition:opacity .15s,background-color .15s;pointer-events:auto}
.b:hover{background:${accent};color:${surface}}
.b:focus-visible{outline:2px solid ${accent};outline-offset:2px}
.b[aria-busy=true]{cursor:progress}
.b svg{width:16px;height:16px;fill:none;stroke:currentColor;stroke-width:2.2;stroke-linecap:round;stroke-linejoin:round}
.w.idle .b:not(:focus-visible){opacity:0;pointer-events:none}
.s{all:initial;position:absolute;top:38px;right:0;width:max-content;max-width:260px;font:12px/1.4 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;color:${ink};border-radius:6px}
.s:not(:empty){padding:6px 10px;background:${surface}f0;box-shadow:0 2px 10px rgba(0,0,0,.35)}
.s.err:not(:empty){background:${danger};color:#fff}
.w.msg .b{visibility:hidden}
@media (prefers-reduced-motion:reduce){.b{transition:none}}
@media (forced-colors:active){.b,.s:not(:empty){border:1px solid CanvasText;forced-color-adjust:auto}}
`;

const SVG_NS = 'http://www.w3.org/2000/svg';

/** A download arrow, built without `innerHTML` so pages that enforce Trusted Types do not refuse it. */
export function icon(doc: Document): SVGElement {
  const svg = doc.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('focusable', 'false');
  const path = doc.createElementNS(SVG_NS, 'path');
  path.setAttribute('d', 'M12 4v11m-5-5 5 5 5-5M5 20h14');
  svg.append(path);
  return svg;
}
