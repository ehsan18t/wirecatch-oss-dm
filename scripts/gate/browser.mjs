// The checks that need a real browser, since jsdom computes no styles and does no layout:
// - e2e-extension: the extension's popup on sample data with every word pseudo-translated longer, laid out by apps/extension/e2e/layout.spec.ts (`pnpm e2e`). It catches a label or a name that sticks out of the popup, scrolls a page sideways, or squeezes text into a sliver.
//
// They need a Chromium-family browser Playwright can drive: Edge by default (installed with Windows), or the one PW_CHANNEL names (on Linux, `PW_CHANNEL=chromium` after `pnpm --filter @wirecatch/extension exec playwright install chromium`). Only a browser that is not installed skips them, and they say so; any other failure to start it fails. CI's Browser jobs set WIRECATCH_REQUIRE_BROWSER=1, since GitHub's Windows and Ubuntu runners both have Edge, so there a skip fails too (docs/ENGINEERING.md section 7: no silent skips).
import { capture, GateError, pnpm } from './process.mjs';

const PROBE = 'apps/extension/scripts/browser-available.mjs';
/** The probe's exit status for "the browser is not installed here". */
const NOT_INSTALLED = 2;
const REQUIRED = process.env.WIRECATCH_REQUIRE_BROWSER === '1';

/** Runs `script` of `pkg` in the browser, or skips it, saying why, where the browser is not installed. */
function inBrowser(what, pkg, script) {
  const probe = capture('node', [PROBE], { allowed: [0, 1, NOT_INSTALLED] });
  const said = [probe.out.trim(), probe.err.trim()].filter((text) => text !== '').join('\n');
  if (probe.status === 1) {
    throw new GateError(`${what} could not start their browser:\n${said}`);
  }
  if (probe.status === NOT_INSTALLED) {
    const hint =
      'Install Edge, or set PW_CHANNEL=chromium and run `pnpm --filter @wirecatch/extension exec playwright install chromium`, to run them.';
    if (REQUIRED) {
      throw new GateError(`WIRECATCH_REQUIRE_BROWSER=1, but ${said}\n${hint}`);
    }
    console.log(`Skipped: no browser for ${what}. ${said}\n${hint}`);
    return;
  }
  pnpm('--filter', pkg, script);
}

export const BROWSER_CHECKS = {
  'e2e-extension': () => {
    inBrowser("the extension's layout checks", '@wirecatch/extension', 'e2e');
  },
};
