import { describe, expect, it } from 'vitest';
import { Requests } from '../src/lib/popup/requests';

describe('popup requests', () => {
  it('numbers requests and settles each one once', async () => {
    const requests = new Requests<string>();
    expect(requests.nextId).toBe(1);
    const first = requests.start();
    const second = requests.start();
    expect([first.id, second.id, requests.nextId]).toEqual([1, 2, 3]);
    requests.settle(second.id, 'two');
    requests.settle(second.id, 'again');
    requests.settle(99, 'unknown');
    expect(await second.result).toBe('two');
    requests.settleAll('gone');
    expect(await first.result).toBe('gone');
  });
});
