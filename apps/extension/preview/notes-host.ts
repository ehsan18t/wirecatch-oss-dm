// The notes preview's website (`notes.html`): the real notes frame placed as the page script places
// it, over a page with colours of its own. The address picks what shows:
// `?notes=started|finished|failed|several&site=light|dark&pseudo`; the browser's scheme sets the notes'.
import { NotesFrame } from '../src/lib/notes/frame';

const params = new URLSearchParams(location.search);
if (params.get('site') === 'dark') document.documentElement.dataset.site = 'dark';
const frame = new NotesFrame({
  container: () => document.documentElement,
  // The notes page reads what to show from the address, as the real one reads its token.
  notesUrl: (token) => `./notes-frame.html${location.search}#${token}`,
  extensionOrigin: location.origin,
  newToken: () => 'preview',
});
frame.mount();
