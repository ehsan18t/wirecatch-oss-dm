// The picture of each settings section, in the navigation and the command palette.

import ArrowRightLeft from '~icons/lucide/arrow-right-left';
import Bell from '~icons/lucide/bell';
import Folder from '~icons/lucide/folder';
import Gauge from '~icons/lucide/gauge';
import Globe from '~icons/lucide/globe';
import ListOrdered from '~icons/lucide/list-ordered';
import Network from '~icons/lucide/network';
import Monitor from '~icons/lucide/monitor';
import Package from '~icons/lucide/package';
import Video from '~icons/lucide/video';
import Wrench from '~icons/lucide/wrench';
import type { IconComponent } from '@wirecatch/ui';
import type { SectionId } from '$lib/settings-sections';

export const SECTION_ICON: Record<SectionId, IconComponent> = {
  general: Monitor,
  notifications: Bell,
  files: Folder,
  browser: Globe,
  media: Video,
  addons: Package,
  speed: Gauge,
  queues: ListOrdered,
  network: Network,
  import_export: ArrowRightLeft,
  advanced: Wrench,
};
