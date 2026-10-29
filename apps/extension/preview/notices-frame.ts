// The notices page of the notices preview (`notices-frame.html`): the real notices, fed sample notices in
// place of the background's, inside the frame `notices-host.ts` placed.
import './notices-frame.css';
import { mount, tick } from 'svelte';
import { isSizeChannel } from '../src/lib/notices/messages';
import { NoticesPage } from '../src/lib/notices/page.svelte';
import Notices from '../src/ui/notices/Notices.svelte';
import { pseudoTranslate } from './pseudo';
import { isSampleNotices, sampleNotices } from './sample-notices';

const params = new URLSearchParams(location.search);
// Like the extension's notices page, it sets no scheme or accent of its own: the browser's scheme,
// since a frame whose scheme differs from the page around it gets an opaque backdrop.
const which = params.get('notices');
const page = new NoticesPage(() => undefined);
window.addEventListener('message', (event) => {
  const [sizes] = event.ports;
  const token = location.hash.slice(1);
  if (isSizeChannel(event.data) && event.data.token === token && sizes) page.sizesTo(sizes);
});
mount(Notices, { target: document.body, props: { page } });
for (const notice of sampleNotices(isSampleNotices(which) ? which : 'several')) page.show(notice);
void tick().then(() => {
  if (params.has('pseudo')) pseudoTranslate(document.body);
});
