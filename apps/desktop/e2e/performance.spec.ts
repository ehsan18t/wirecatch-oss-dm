// Acceptance item 2: the list stays smooth at 10,000 downloads.
//
// Loads 10,000 sample downloads with 60 of them running (60 rows change four times a second),
// then measures frame times with requestAnimationFrame while (a) idle and (b) scrolling the
// list continuously, and records long tasks. Results go to test-results/performance.json and the test's attachments.

import { mkdirSync, writeFileSync } from 'node:fs';
import { expect, test } from '@playwright/test';
import { open, rows } from './helpers';

interface FrameStats {
  frames: number;
  p50: number;
  p95: number;
  p99: number;
  max: number;
  over33ms: number;
  fps: number;
  longTasks: number;
  longestTaskMs: number;
}

test('10,000 downloads stay smooth while updating and scrolling', async ({ page }, testInfo) => {
  const navStart = Date.now();
  await open(page, process.env.PERF_QUERY ?? 'count=10000&active=60&capture=0');
  await rows(page).first().waitFor();
  const firstRowsMs = Date.now() - navStart;

  const domRows = await rows(page).count();
  const total = await page.getByRole('grid', { name: 'Downloads' }).getAttribute('aria-rowcount');

  const measure = (scroll: boolean) =>
    page.evaluate(async (doScroll): Promise<FrameStats> => {
      const list = document.getElementById('download-list')!;
      const max = list.scrollHeight - list.clientHeight;
      let pos = list.scrollTop;
      const deltas: number[] = [];
      let longTasks = 0;
      let longest = 0;
      const obs = new PerformanceObserver((l) => {
        for (const e of l.getEntries()) {
          longTasks += 1;
          longest = Math.max(longest, e.duration);
        }
      });
      try {
        obs.observe({ type: 'longtask', buffered: false });
      } catch {
        // Long task timing is not available everywhere.
      }
      const duration = 5000;
      await new Promise<void>((resolve) => {
        const start = performance.now();
        let last = start;
        let dir = 1;
        const frame = (now: number) => {
          deltas.push(now - last);
          last = now;
          if (doScroll) {
            // Write-only, like a wheel or scrollbar drag; reading layout here would distort it.
            pos += dir * 180;
            if (pos >= max) dir = -1;
            if (pos <= 0) dir = 1;
            list.scrollTop = pos;
          }
          if (now - start < duration) requestAnimationFrame(frame);
          else resolve();
        };
        requestAnimationFrame(frame);
      });
      obs.disconnect();
      deltas.shift();
      const sorted = [...deltas].sort((a, b) => a - b);
      const at = (p: number) =>
        sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))] ?? 0;
      const round = (n: number) => Math.round(n * 100) / 100;
      return {
        frames: deltas.length,
        p50: round(at(0.5)),
        p95: round(at(0.95)),
        p99: round(at(0.99)),
        max: round(sorted.at(-1) ?? 0),
        over33ms: deltas.filter((d) => d > 33.4).length,
        fps: round(deltas.length / (duration / 1000)),
        longTasks,
        longestTaskMs: round(longest),
      };
    }, scroll);

  const idle = await measure(false);
  const scrolling = await measure(true);
  const results = {
    firstRowsMs,
    domRows,
    total,
    idleWithUpdates: idle,
    scrollingWithUpdates: scrolling,
  };
  const report = JSON.stringify(results, null, 2);
  await testInfo.attach('performance', { body: report, contentType: 'application/json' });
  mkdirSync('test-results', { recursive: true });
  writeFileSync('test-results/performance.json', report);

  expect(total).toBe('10000');
  // Virtualized: only a screenful of rows exists in the page.
  expect(domRows).toBeLessThan(60);
  expect(scrolling.p95).toBeLessThan(34);
  expect(idle.p95).toBeLessThan(34);
});
