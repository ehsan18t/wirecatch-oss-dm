// The notes page of the notes preview (`notes-frame.html`): the real notes, fed sample notes in
// place of the background's, inside the frame `notes-host.ts` placed.
import './notes-frame.css';
import { mount, tick } from 'svelte';
import { isSizeChannel } from '../src/lib/notes/messages';
import { NotesPage } from '../src/lib/notes/page.svelte';
import Notes from '../src/ui/notes/Notes.svelte';
import { pseudoTranslate } from './pseudo';
import { isSampleNotes, sampleNotes } from './sample-notes';

const params = new URLSearchParams(location.search);
// Like the extension's notes page, it sets no scheme or accent of its own: the browser's scheme,
// since a frame whose scheme differs from the page around it gets an opaque backdrop.
const which = params.get('notes');
const page = new NotesPage(() => undefined);
window.addEventListener('message', (event) => {
  const [sizes] = event.ports;
  const token = location.hash.slice(1);
  if (isSizeChannel(event.data) && event.data.token === token && sizes) page.sizesTo(sizes);
});
mount(Notes, { target: document.body, props: { page } });
for (const note of sampleNotes(isSampleNotes(which) ? which : 'several')) page.show(note);
void tick().then(() => {
  if (params.has('pseudo')) pseudoTranslate(document.body);
});
