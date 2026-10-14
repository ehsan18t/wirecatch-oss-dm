import { questionId, type UiEvent } from '@wirecatch/protocol';
import { describe, expect, it, vi } from 'vitest';
import { appState, mockBackend, windowState } from '$lib/testing/setup';
import { dispatch, HeldEvents, type UiEventHandlers } from './events';

const FORWARD: UiEvent = { type: 'window_requested' };
const STORAGE: UiEvent = { type: 'storage_failed', data: { message: 'Disk full.', fix: null } };

describe('HeldEvents', () => {
  it('holds events until released, then handles them in order and the rest as they come', () => {
    const seen: string[] = [];
    const events = new HeldEvents((e) => seen.push(e.type));
    events.receive(STORAGE);
    events.receive(FORWARD);
    expect(seen).toEqual([]);
    events.release();
    expect(seen).toEqual(['storage_failed', 'window_requested']);
    events.receive(STORAGE);
    expect(seen).toEqual(['storage_failed', 'window_requested', 'storage_failed']);
  });

  it('drops what it held when the load it waited for was overtaken', () => {
    const seen: string[] = [];
    const events = new HeldEvents((e) => seen.push(e.type));
    events.receive(STORAGE);
    events.drop();
    events.receive(FORWARD);
    expect(seen).toEqual(['window_requested']);
  });
});

describe('event handler tables', () => {
  it('run the handler for the event type, and only that one', () => {
    const handlers: UiEventHandlers = {
      downloads_changed: vi.fn(),
      downloads_removed: vi.fn(),
      queues_changed: vi.fn(),
      settings_changed: vi.fn(),
      captured: vi.fn(),
      confirmation_requested: vi.fn(),
      storage_failed: vi.fn(),
      settings_imported: vi.fn(),
      media_choice_requested: vi.fn(),
      media_analysing: vi.fn(),
      addons_changed: vi.fn(),
      suffix_list_checked: vi.fn(),
      addon_required: vi.fn(),
      confirmation_closed: vi.fn(),
      window_requested: vi.fn(),
    };
    dispatch(handlers, STORAGE);
    dispatch(handlers, FORWARD);
    expect(handlers.storage_failed).toHaveBeenCalledExactlyOnceWith(STORAGE);
    expect(handlers.window_requested).toHaveBeenCalledExactlyOnceWith(FORWARD);
    const others = Object.entries(handlers).filter(
      ([type]) => type !== 'storage_failed' && type !== 'window_requested',
    );
    for (const [, handler] of others) expect(handler).not.toHaveBeenCalled();
  });

  it('keep a storage failure in the main window, and change nothing for a window request', async () => {
    const app = appState();
    await app.init();
    const notes = app.toasts.items.length;
    app.handle(FORWARD);
    expect(app.storageProblem).toBeNull();
    app.handle(STORAGE);
    expect(app.storageProblem).toBe('Disk full.');
    expect(app.toasts.items.length).toBe(notes);
  });

  it('leave a question window alone for events about another question', async () => {
    const backend = mockBackend({ empty: true });
    backend.simulateConfirmation();
    const [asked] = await backend.pendingConfirmations();
    const id = asked?.id ?? questionId(0);
    const { win } = windowState(backend, { question: id });
    await win.init();
    expect(win.question?.kind).toBe('capture');
    win.handle({
      type: 'media_analysing',
      data: { id: questionId(id + 1), url: 'https://example.com/v' },
    });
    win.handle({ type: 'confirmation_closed', data: { id: questionId(id + 1) } });
    win.handle(STORAGE);
    expect(win.phase).toBe('ask');
    expect(win.question?.kind).toBe('capture');
    win.handle({ type: 'confirmation_closed', data: { id } });
    expect(win.phase).toBe('gone');
  });
});
