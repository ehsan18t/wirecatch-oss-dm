import './style.css';
import { mount } from 'svelte';
import { browser } from 'wxt/browser';
import { installDiagnostics } from '@/lib/diag/install';
import { log } from '@/lib/diag/log';
import { isSizeChannel, isToNoticesPage, NOTICES_PORT } from '@/lib/notices/messages';
import { NoticesPage } from '@/lib/notices/page.svelte';
import Notices from '@/ui/notices/Notices.svelte';

installDiagnostics('notices', window);

// The page script that framed this page put the token after the `#`; the background gives notices
// only to a page that proves it holds the token of a frame it asked for.
const token = location.hash.slice(1);
const port = browser.runtime.connect({ name: NOTICES_PORT });
let connected = true;
const page = new NoticesPage((message) => {
  if (connected) port.postMessage(message);
});
port.onMessage.addListener((message: unknown) => {
  if (!isToNoticesPage(message)) {
    log.warn('notices', 'The background sent a message the notices page does not know');
    return;
  }
  if (message.type === 'notice') page.show(message.notice);
  else page.withdraw(message.id);
});
// The background restarted or let this frame go: its notices' buttons lead nowhere now, so they go,
// and the frame shrinks to nothing; the background places a fresh one for the next notice.
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

mount(Notices, { target: document.body, props: { page } });
