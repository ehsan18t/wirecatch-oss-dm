// The preview page: the popup or the options page on sample data, in a normal browser tab, for
// reviewing the design. The address picks what shows:
// `?page=popup|options&state=connected|capture-off|many|closed|missing|cached|connecting|long&scheme=light|dark&accent=<name>`,
// `&pseudo` to lengthen every word as a translation may (preview/pseudo.ts), and `&font=dejavu` to
// set it in DejaVu Sans, as Linux shows it (style.css).
import './style.css';
import { applyAppearance, DEFAULT_ACCENT, isAccent, isScheme } from '@wirecatch/ui';
import { mount } from 'svelte';
import Options from '../src/ui/options/Options.svelte';
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
  material: null,
});

const model = sampleModel(isSampleState(state) ? state : 'connected');
const noop = (): void => undefined;

if (params.get('page') === 'options') {
  mount(Options, {
    target: document.body,
    props: { model, mediaShortcut: 'Alt+Shift+D', openShortcuts: noop },
  });
} else {
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
      siteAccess: state !== 'cached',
      openSettings: noop,
    },
  });
}

if (params.has('pseudo')) pseudoTranslate(document.body);
