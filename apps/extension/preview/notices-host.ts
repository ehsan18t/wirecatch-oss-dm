// The notices preview's website (`notices.html`): the real notices frame placed as the page script places
// it, over a page with colours of its own. The address picks what shows:
// `?notices=started|finished|failed|several&site=light|dark&pseudo`; the browser's scheme sets the notices'.
import { NoticesFrame } from '../src/lib/notices/frame';

const params = new URLSearchParams(location.search);
if (params.get('site') === 'dark') document.documentElement.dataset.site = 'dark';
const frame = new NoticesFrame({
  container: () => document.documentElement,
  // The notices page reads what to show from the address, as the real one reads its token.
  noticesUrl: (token) => `./notices-frame.html${location.search}#${token}`,
  extensionOrigin: location.origin,
  newToken: () => 'preview',
});
frame.mount();
