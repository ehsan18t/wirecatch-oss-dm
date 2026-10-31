// Answers whether Playwright can start the browser the end-to-end tests use, so the gate can skip them only where that browser is not installed. Exit 0: it started. Exit 2: it is not installed on this machine, and the message says which. Anything else (a missing or broken Playwright, a launch that fails or times out) exits 1 with the reason: that is a fault to fix, never a reason to skip.
import { chromium } from '@playwright/test';

// The same choice as playwright.config.ts: Edge, installed with Windows, unless PW_CHANNEL names another.
const channel = process.env.PW_CHANNEL ?? 'msedge';

/** What Playwright says when the browser's executable is not on the machine. */
const NOT_INSTALLED = /is not found at|Executable doesn't exist/;

try {
  const browser = await chromium.launch({ channel });
  await browser.close();
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  const notInstalled = NOT_INSTALLED.test(message);
  console.log(
    `Playwright could not start ${channel}${notInstalled ? ' (not installed)' : ''}: ${message.split('\n')[0]}`,
  );
  process.exit(notInstalled ? 2 : 1);
}
