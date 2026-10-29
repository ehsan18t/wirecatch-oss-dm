import { getConfig } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { PATIENCE_MS } from './jsdom';

describe('test waits', () => {
  it("give Testing Library's waits the same patience as the Rust tests", () => {
    expect(getConfig().asyncUtilTimeout).toBe(PATIENCE_MS);
  });
});
