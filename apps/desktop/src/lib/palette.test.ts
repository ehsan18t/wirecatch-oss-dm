// The command palette finds actions and settings by the words people use.

import { describe, expect, it } from 'vitest';
import { buildCommands, searchCommands } from './commands';
import { setupWithUser } from './testing/setup';

describe('the palette', () => {
  it('can retry all failed downloads and finds settings by common words', async () => {
    const { app } = await setupWithUser();
    const all = buildCommands(app);
    expect(all.some((c) => c.id === 'dl.retryfailed')).toBe(true);
    for (const word of ['windows', 'startup', 'login']) {
      expect(searchCommands(all, word).some((c) => c.id === 'setting.start_with_system')).toBe(
        true,
      );
    }
    await app.actions.retryAllFailed();
    expect(app.counts.status.failed).toBe(0);
  });

  it('does not match letters scattered across a word', async () => {
    const { app } = await setupWithUser();
    app.selection.only(app.visible[0]!);
    const titles = searchCommands(buildCommands(app), 'move').map((c) => c.title);
    expect(titles.length).toBeGreaterThan(0);
    for (const t of titles) expect(t).not.toMatch(/^Resume|^Remove/);
    expect(titles.every((t) => /\bmove/i.test(t))).toBe(true);
  });
});
