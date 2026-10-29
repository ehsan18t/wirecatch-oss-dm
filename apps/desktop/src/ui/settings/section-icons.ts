// The picture of each settings section, in the navigation and the command palette.

import Bell from '~icons/lucide/bell';
import Folder from '~icons/lucide/folder';
import Gauge from '~icons/lucide/gauge';
import Globe from '~icons/lucide/globe';
import Network from '~icons/lucide/network';
import Monitor from '~icons/lucide/monitor';
import Video from '~icons/lucide/video';
import type { IconComponent } from '@wirecatch/ui';
import type { SectionId } from '$lib/settings-sections';

export const SECTION_ICON: Record<SectionId, IconComponent> = {
  general: Monitor,
  notifications: Bell,
  files: Folder,
  browser: Globe,
  media: Video,
  speed: Gauge,
  network: Network,
};
