// The picture of each kind of file, in the list, the sidebar and the details.

import AppWindow from '~icons/lucide/app-window';
import Archive from '~icons/lucide/archive';
import File from '~icons/lucide/file';
import FileText from '~icons/lucide/file-text';
import Film from '~icons/lucide/film';
import Image from '~icons/lucide/image';
import Music from '~icons/lucide/music';
import type { Category } from '@wirecatch/protocol';
import type { IconComponent } from '@wirecatch/ui';

export const CATEGORY_ICON: Record<Category, IconComponent> = {
  video: Film,
  audio: Music,
  archive: Archive,
  document: FileText,
  program: AppWindow,
  image: Image,
  other: File,
};
