<script lang="ts">
  import AppWindow from '@lucide/svelte/icons/app-window';
  import Archive from '@lucide/svelte/icons/archive';
  import File from '@lucide/svelte/icons/file';
  import FileText from '@lucide/svelte/icons/file-text';
  import Film from '@lucide/svelte/icons/film';
  import Image from '@lucide/svelte/icons/image';
  import Music from '@lucide/svelte/icons/music';
  import type { Category } from '@wirecatch/protocol';
  import Icon from './Icon.svelte';
  import type { IconSize } from './scale';

  let { category, size = 'md' }: { category: Category; size?: IconSize } = $props();

  const icons = {
    video: Film,
    audio: Music,
    archive: Archive,
    document: FileText,
    program: AppWindow,
    image: Image,
    other: File,
  };
</script>

<Icon icon={icons[category]} {size} thin />
