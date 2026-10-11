// Not part of the acceptance run: a CPU profile of scrolling 10,000 rows with live updates,
// summarized by self time per function and attached to the test report. Run with PROFILE=1 to
// investigate slow frames.

import { test } from '@playwright/test';
import { open, rows } from './helpers';

interface Node {
  id: number;
  callFrame: { functionName: string; url: string; lineNumber: number };
}

test('profile scrolling', async ({ page }, testInfo) => {
  test.skip(!process.env.PROFILE, 'set PROFILE=1 to run');
  await open(page, process.env.PERF_QUERY ?? 'count=10000&active=60&capture=0');
  await rows(page).first().waitFor();
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Profiler.enable');
  await cdp.send('Profiler.setSamplingInterval', { interval: 200 });
  await cdp.send('Profiler.start');
  await page.evaluate(async () => {
    const list = document.getElementById('download-list')!;
    const max = list.scrollHeight - list.clientHeight;
    let pos = 0;
    await new Promise<void>((resolve) => {
      const start = performance.now();
      let dir = 1;
      const frame = (now: number) => {
        pos += dir * 180;
        if (pos >= max) dir = -1;
        if (pos <= 0) dir = 1;
        list.scrollTop = pos;
        if (now - start < 5000) requestAnimationFrame(frame);
        else resolve();
      };
      requestAnimationFrame(frame);
    });
  });
  const { profile } = (await cdp.send('Profiler.stop')) as unknown as {
    profile: { nodes: Node[]; samples: number[]; timeDeltas: number[] };
  };
  const byId = new Map(profile.nodes.map((n) => [n.id, n]));
  const self = new Map<string, number>();
  profile.samples.forEach((id, i) => {
    const n = byId.get(id);
    if (!n) return;
    const f = n.callFrame;
    const key = `${f.functionName || '(anonymous)'} ${f.url.split('/').pop()}:${f.lineNumber}`;
    self.set(key, (self.get(key) ?? 0) + (profile.timeDeltas[i] ?? 0) / 1000);
  });
  const top = [...self].sort((a, b) => b[1] - a[1]).slice(0, 25);
  const summary = top.map(([k, ms]) => `${ms.toFixed(1).padStart(8)} ms  ${k}`).join('\n');
  await testInfo.attach('self time', { body: summary, contentType: 'text/plain' });
});
