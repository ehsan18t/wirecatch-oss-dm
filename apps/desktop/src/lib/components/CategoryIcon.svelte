<script lang="ts">
  import AppWindow from '@lucide/svelte/icons/app-window';
  import Archive from '@lucide/svelte/icons/archive';
  import File from '@lucide/svelte/icons/file';
  import FileText from '@lucide/svelte/icons/file-text';
  import Film from '@lucide/svelte/icons/film';
  import Image from '@lucide/svelte/icons/image';
  import Music from '@lucide/svelte/icons/music';
  import type { Category } from '@wirecatch/protocol';

  let { category, size = 16 }: { category: Category; size?: number } = $props();

  const icons = {
    video: Film,
    audio: Music,
    archive: Archive,
    document: FileText,
    program: AppWindow,
    image: Image,
    other: File,
  };

  const Icon = $derived(icons[category]);
</script>

<Icon {size} strokeWidth={1.75} aria-hidden="true" />
