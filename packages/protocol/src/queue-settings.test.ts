import { describe, expect, it } from 'vitest';
import { isQueueSettings } from './generated/validators';
import { GLOBAL, newQueueSettings, ownSetting, ownValue } from './queue-settings';

describe('queue settings', () => {
  it('reads an own value and none for a setting that follows the app', () => {
    expect(ownValue(ownSetting(3))).toBe(3);
    expect(ownValue(ownSetting(0))).toBe(0);
    expect(ownValue<number>(GLOBAL)).toBeNull();
  });

  it('builds valid default queue settings with no icon', () => {
    const settings = newQueueSettings('Videos');
    expect(settings.icon).toBeNull();
    expect(isQueueSettings(settings)).toBe(true);
  });
});
