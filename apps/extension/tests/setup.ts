// The design system's jsdom.ts adds the browser features jsdom leaves out that its components use
// (native dialogs, popovers); this adds what only the extension's page tests need.
import { stubMatchMedia } from '@wirecatch/ui/testing/jsdom';

// No query matches: full motion, the light scheme.
stubMatchMedia(() => false);
