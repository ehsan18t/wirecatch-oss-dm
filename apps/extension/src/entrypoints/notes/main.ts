import './style.css';
import { mount } from 'svelte';
import { browser } from 'wxt/browser';
import { installDiagnostics } from '@/lib/diag/install';
import { log } from '@/lib/diag/log';
import { isSizeChannel, isToNotesPage, NOTES_PORT } from '@/lib/notes/messages';
import { NotesPage } from '@/lib/notes/page.svelte';
import Notes from '@/ui/notes/Notes.svelte';

installDiagnostics('notes', window);

// The page script that framed this page put the token after the `#`; the background gives notes
// only to a page that proves it holds the token of a frame it asked for.
const token = location.hash.slice(1);
const port = browser.runtime.connect({ name: NOTES_PORT });
let connected = true;
const page = new NotesPage((message) => {
  if (connected) port.postMessage(message);
});
port.onMessage.addListener((message: unknown) => {
  if (isToNotesPage(message)) page.show(message.note);
  else log.warn('notes', 'The background sent a message the notes page does not know');
});
// The background restarted or let this frame go: its notes' buttons lead nowhere now, so they go,
// and the frame shrinks to nothing; the background places a fresh one for the next note.
port.onDisconnect.addListener(() => {
  connected = false;
  page.lost();
});
port.postMessage({ type: 'hello', token });

// The page script around hands over a channel of its own for sizes, naming this frame's token,
// which no website knows.
window.addEventListener('message', (event) => {
  const [sizes] = event.ports;
  if (isSizeChannel(event.data) && event.data.token === token && sizes) page.sizesTo(sizes);
});

mount(Notes, { target: document.body, props: { page } });
