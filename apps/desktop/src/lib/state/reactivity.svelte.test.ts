import { flushSync } from 'svelte';
import { describe, expect, it } from 'vitest';
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
