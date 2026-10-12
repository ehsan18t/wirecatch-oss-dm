import { flushSync } from 'svelte';
import { describe, expect, it } from 'vitest';
import { appState, mockBackend } from '$lib/testing/setup';
import { track } from './reactivity';

describe('track', () => {
  it('runs an effect again when a value it only tracks changes', () => {
    const state = $state({ view: 'all', search: '' });
    let runs = 0;
    const cleanup = $effect.root(() => {
      $effect(() => {
        track(state.view, state.search);
        runs++;
      });
    });
    flushSync();
    expect(runs).toBe(1);
    state.search = 'zip';
    flushSync();
    expect(runs).toBe(2);
    state.view = 'queue';
    flushSync();
    expect(runs).toBe(3);
    cleanup();
  });
});

describe('state loaded from the sample backend', () => {
  // The mock hands out structuredClone copies. If those came from another realm (as they do in
  // Vitest's vm pools), Svelte would leave them as they are, and the tests would see a shallowly
  // reactive app while the real one is deeply reactive.
  it('is deeply reactive, so a nested write runs effects again', async () => {
    const backend = mockBackend();
    const app = appState(backend);
    await app.init();
    const queue = app.queues.list[0];
    if (!queue) throw new Error('the sample has no queue');
    const seen: string[] = [];
    const cleanup = $effect.root(() => {
      $effect(() => {
        seen.push(queue.name);
      });
    });
    flushSync();
    queue.name = 'Renamed';
    flushSync();
    expect(seen.at(-1)).toBe('Renamed');
    expect(seen).toHaveLength(2);
    cleanup();
  });
});
