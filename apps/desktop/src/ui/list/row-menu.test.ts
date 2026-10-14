import type { DownloadSummary } from '@wirecatch/protocol';
import type { MenuAction, MenuEntry } from '@wirecatch/ui';
import { describe, expect, it } from 'vitest';
import { renderApp, settle } from '$lib/testing/setup';
import { rowMenu } from './row-menu';

const actions = (entries: MenuEntry[]): MenuAction[] =>
  entries.filter((e): e is MenuAction => !('kind' in e));

async function menuFor(test: (row: DownloadSummary) => boolean) {
  const { app } = await renderApp();
  const row = app.visible.map((id) => app.downloads.get(id)).find((r) => r && test(r));
  if (!row) throw new Error('no such sample download');
  app.selection.only(row.id);
  await settle();
  return { app, row, entries: actions(rowMenu(app)) };
}

const labels = (entries: MenuAction[]) => entries.map((e) => e.label);

describe('the list context menu', () => {
  it("leads with a failed download's own fix, which Enter runs too", async () => {
    const { entries } = await menuFor((r) => r.error?.fix === 'keep_original');
    expect(entries[0]?.label).toBe('Keep the original');
    expect(entries[0]?.shortcut).toEqual(['Enter']);
    // Trying again is still offered, after the fix.
    expect(labels(entries)).toContain('Try again');
  });

  it('offers Try again once when trying again is the fix', async () => {
    const { entries } = await menuFor((r) => r.error?.fix === 'retry');
    expect(entries[0]?.label).toBe('Try again');
    expect(labels(entries).filter((l) => l === 'Try again')).toHaveLength(1);
  });

  it('runs the fix it offers', async () => {
    const { app, row, entries } = await menuFor((r) => r.error?.fix === 'retry');
    entries[0]?.onselect();
    await settle();
    expect(app.downloads.get(row.id)?.status).toBe('queued');
  });

  it('offers a new link for a download still to come, in the words the details use', async () => {
    const { entries } = await menuFor((r) => r.status === 'downloading');
    expect(labels(entries)).toContain('Get a new link');
  });
});
