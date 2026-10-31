import { describe, expect, it, vi } from 'vitest';
import { announcer } from '../src/lib/media/announce';
import { MEDIA_ANNOUNCE } from '../src/lib/media/messages';
import { flush } from './fakes';

function announce(sent: Promise<unknown>) {
  const sendMessage = vi.fn(() => sent);
  const notify = vi.fn();
  return { run: announcer({ sendMessage, notify }), sendMessage, notify };
}

describe('announcer', () => {
  it("shows the answer in the page's top frame", async () => {
    const { run, sendMessage, notify } = announce(Promise.resolve());
    run(7, { ok: false, message: 'Wirecatch did not accept it.' });
    await flush();
    expect(sendMessage).toHaveBeenCalledWith(
      7,
      { type: MEDIA_ANNOUNCE, ok: false, message: 'Wirecatch did not accept it.' },
      { frameId: 0 },
    );
    expect(notify).not.toHaveBeenCalled();
  });

  it('puts a failure in the popup when the page cannot show it', async () => {
    const { run, notify } = announce(Promise.reject(new Error('no receiver')));
    run(7, { ok: false, message: 'Wirecatch did not accept it.' });
    await flush();
    expect(notify).toHaveBeenCalledWith('error', 'Wirecatch did not accept it.');
  });

  it('raises no alert for a download the app took', async () => {
    const { run, notify } = announce(Promise.reject(new Error('no receiver')));
    run(7, { ok: true, message: 'Sent to Wirecatch.' });
    await flush();
    expect(notify).not.toHaveBeenCalled();
  });
});
