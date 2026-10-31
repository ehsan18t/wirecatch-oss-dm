/** The content scripts' entry points: where they run, and what they set up and take down. */
import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from 'vitest';
import type { ContentScriptContext } from 'wxt/utils/content-script-context';
import { fakeBrowser } from 'wxt/testing/fake-browser';
import media from '../src/entrypoints/media.content';
import noticeFrame from '../src/entrypoints/notice-frame.content';
import { MediaButton } from '../src/lib/media/button';
import {
  MEDIA_ANNOUNCE,
  MEDIA_BUTTON_QUERY,
  MEDIA_BUTTON_SETTING,
} from '../src/lib/media/messages';
import { SITE_ORIGINS } from '../src/lib/site-access';
import { flush } from './fakes';

/** Runs the media script's `main`; the returned function invalidates it, as a reloaded extension does. */
function start(): () => void {
  const cleanups: (() => void)[] = [];
  const ctx = {
    onInvalidated: (cleanup: () => void) => {
      cleanups.push(cleanup);
      return () => undefined;
    },
  };
  void media.main(ctx as unknown as ContentScriptContext);
  return () => {
    for (const cleanup of cleanups) cleanup();
  };
}

/** Stubs `runtime.sendMessage`: the fake browser passes no answer back. */
function answerWith(
  answer: Promise<unknown>,
): MockInstance<(message: unknown) => Promise<unknown>> {
  const send = vi.spyOn(fakeBrowser.runtime, 'sendMessage') as unknown as MockInstance<
    (message: unknown) => Promise<unknown>
  >;
  return send.mockReturnValue(answer);
}

/** A message from the background to this page's scripts, which answer none of them. */
async function deliver(message: unknown): Promise<void> {
  const answers = await fakeBrowser.runtime.onMessage.trigger(message, {}, () => undefined);
  expect(answers.every((answer) => answer === undefined)).toBe(true);
}

describe('the content scripts', () => {
  it('run in every web page, the media button in every frame too', () => {
    expect(media.matches).toEqual(SITE_ORIGINS);
    expect(media.allFrames).toBe(true);
    expect(noticeFrame.matches).toEqual(SITE_ORIGINS);
    expect(noticeFrame.allFrames).toBeUndefined();
  });
});

describe('the media script', () => {
  let enabled: MockInstance<(on: boolean) => void>;

  beforeEach(() => {
    fakeBrowser.reset();
    enabled = vi.spyOn(MediaButton.prototype, 'setEnabled');
  });

  afterEach(() => {
    fakeBrowser.reset();
  });

  it('shows the button as the background says', async () => {
    const sent = answerWith(Promise.resolve({ type: MEDIA_BUTTON_SETTING, on: false }));
    const stop = start();
    await flush();
    expect(sent).toHaveBeenCalledWith({ type: MEDIA_BUTTON_QUERY });
    expect(enabled).toHaveBeenLastCalledWith(false);
    stop();
  });

  it('shows the button when the background cannot say', async () => {
    answerWith(Promise.reject(new Error('no background')));
    const stop = start();
    await flush();
    expect(enabled).toHaveBeenLastCalledWith(true);
    stop();
  });

  it('follows the setting and the shortcut answers until it is invalidated', async () => {
    const announce = vi.spyOn(MediaButton.prototype, 'announce');
    const destroy = vi.spyOn(MediaButton.prototype, 'destroy');
    const stop = start();
    await flush();
    await deliver({ type: MEDIA_BUTTON_SETTING, on: false });
    expect(enabled).toHaveBeenLastCalledWith(false);
    await deliver({
      type: MEDIA_ANNOUNCE,
      ok: false,
      message: 'Wirecatch did not accept it.',
    });
    expect(announce).toHaveBeenCalledWith('Wirecatch did not accept it.', true);

    stop();
    expect(destroy).toHaveBeenCalledOnce();
    enabled.mockClear();
    await deliver({ type: MEDIA_BUTTON_SETTING, on: true });
    expect(enabled).not.toHaveBeenCalled();
  });
});
