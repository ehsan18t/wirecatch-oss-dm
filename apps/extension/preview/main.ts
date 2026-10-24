// The preview page: the popup on sample data, in a normal browser tab, for reviewing the design.
// The address picks what shows:
// `?state=connected|no-media|capture-off|many|closed|missing|no-access|connecting|long|old-app&scheme=light|dark&accent=<name>`,
// `&open` to unfold the Downloads and Video and audio sections, `&pseudo` to lengthen every word as
// a translation may (preview/pseudo.ts), and `&font=dejavu` to set it in DejaVu Sans, as Linux
// shows it (style.css).
import './style.css';
import { applyAppearance, DEFAULT_ACCENT, isAccent, isScheme } from '@wirecatch/ui';
import { mount, tick } from 'svelte';
import Popup from '../src/ui/popup/Popup.svelte';
import { pseudoTranslate } from './pseudo';
import { isSampleState, LONG_PAGE_URL, sampleModel } from './sample';

const params = new URLSearchParams(location.search);
const scheme = params.get('scheme');
const accent = params.get('accent');
const state = params.get('state');
if (params.get('font') === 'dejavu') document.documentElement.dataset.font = 'dejavu';
applyAppearance(document.documentElement, {
  scheme: isScheme(scheme) ? scheme : null,
  accent: isAccent(accent) ? accent : DEFAULT_ACCENT,
});

const model = sampleModel(isSampleState(state) ? state : 'connected');
const noop = (): void => undefined;

model.watchMedia(1);
const frame = document.createElement('div');
// Where a browser draws the popup: under its toolbar button, on the page's colours.
frame.className = 'popup-frame';
document.body.append(frame);
mount(Popup, {
  target: frame,
  props: {
    model,
    currentUrl: state === 'long' ? LONG_PAGE_URL : 'https://www.example.com/downloads',
    mediaShortcut: 'Alt+Shift+D',
    openShortcuts: noop,
    siteAccess: state !== 'no-access',
  },
});

/** The popup's folded sections, by the label each starts with. */
const FOLDS = ['Downloads', 'Video and audio'];

void (async () => {
  await model.loaded(true);
  await tick();
  if (params.has('open')) {
    for (const button of frame.querySelectorAll('button[aria-expanded]')) {
      const label = button.textContent.trim();
      if (button instanceof HTMLButtonElement && FOLDS.some((fold) => label.startsWith(fold))) {
        button.click();
      }
    }
  }
  if (params.has('pseudo')) pseudoTranslate(document.body);
})();
