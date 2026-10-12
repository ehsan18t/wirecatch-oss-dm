import { describe, expect, it } from 'vitest';
import { mockOptionsFromUrl } from '.';

describe('mockOptionsFromUrl', () => {
  it('reads every option from the address bar', () => {
    const options = mockOptionsFromUrl('?count=10000&active=60&seed=3&latency=300&empty&capture=0');
    expect(options).toMatchObject({
      count: 10_000,
      active: 60,
      seed: 3,
      latencyMs: 300,
      empty: true,
      autoCapture: false,
    });
  });

  it('leaves out what is missing or not a number', () => {
    const options = mockOptionsFromUrl('?latency=soon&count=');
    expect(options.latencyMs).toBeUndefined();
    expect(options.count).toBeUndefined();
    expect(options.empty).toBe(false);
    expect(options.autoCapture).toBe(true);
    expect(options.now).toBeUndefined();
  });

  it('runs the sample clock from a time of day', () => {
    const now = mockOptionsFromUrl('?at=12:00').now;
    expect(now).toBeTypeOf('function');
    const at = new Date(now?.() ?? 0);
    expect([at.getHours(), at.getMinutes()]).toEqual([12, 0]);
  });
});
