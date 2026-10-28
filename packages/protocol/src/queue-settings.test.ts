import { describe, expect, it } from 'vitest';
import { FOLLOW_APP, ownSetting, ownValue } from './queue-settings';

describe('queue settings', () => {
  it('reads an own value and none for a setting that follows the app', () => {
    expect(ownValue(ownSetting(3))).toBe(3);
    expect(ownValue(ownSetting(0))).toBe(0);
    expect(ownValue<number>(FOLLOW_APP)).toBeNull();
  });
});
