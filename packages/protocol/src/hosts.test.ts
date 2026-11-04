// The cases are shared with `wirecatch_protocol::hosts`.

import { describe, expect, it } from 'vitest';
import CASES from './hosts.cases.json';
import { hostOnSite } from './hosts';

describe('hostOnSite', () => {
  it.each(CASES.on_site.map(({ host, site }) => [host, site]))('%s is on %s', (host, site) => {
    expect(hostOnSite(host, site)).toBe(true);
  });

  it.each(CASES.not_on_site.map(({ host, site }) => [host, site]))(
    '%s is not on %s',
    (host, site) => {
      expect(hostOnSite(host, site)).toBe(false);
    },
  );
});
