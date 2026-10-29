// Starting the app again, or stopping it early, leaves each listener once.

import { describe, expect, it, vi } from 'vitest';
import type { MockBackend } from './backend/mock';
import { appState, mockBackend, settle, setupWithUser } from './testing/setup';

describe('Try again does not double the listeners', () => {
  it('init twice still handles each event once', async () => {
    const { app, backend } = await setupWithUser();
    await app.init();
    await settle();
    const asked = vi.spyOn(app.questions, 'confirmationRequested');
    backend.simulateCapture();
    await settle();
    expect(asked).toHaveBeenCalledTimes(1);
  });

  it('dispose before the backend is ready leaves nothing listening', async () => {
    const backend = mockBackend();
    const ready = Promise.withResolvers<MockBackend>();
    const app = appState(() => ready.promise);
    const started = app.init();
    app.dispose();
    ready.resolve(backend);
    await started;
    backend.simulateCapture();
    expect(app.toasts.items).toHaveLength(0);
    expect(app.ready).toBe(false);
  });
});
