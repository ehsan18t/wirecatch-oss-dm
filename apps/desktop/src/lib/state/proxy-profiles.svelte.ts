// The saved proxy profiles (D73), as the core last sent them.

import type { ProxyProfile } from '@wirecatch/protocol';

export class ProxyProfiles {
  list = $state<ProxyProfile[]>([]);
}
