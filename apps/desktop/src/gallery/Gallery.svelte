<script lang="ts">
  // The gallery is laid out as the app will be: the frameless shell, the sidebar, and the content on its layer.
  import Blocks from '~icons/lucide/blocks';
  import Layers from '~icons/lucide/layers';
  import MousePointerClick from '~icons/lucide/mouse-pointer-click';
  import Moon from '~icons/lucide/moon';
  import Palette from '~icons/lucide/palette';
  import PanelLeft from '~icons/lucide/panel-left';
  import Search from '~icons/lucide/search';
  import Sparkles from '~icons/lucide/sparkles';
  import Sun from '~icons/lucide/sun';
  import {
    applyAppearance,
    DEFAULT_ACCENT,
    isAccent,
    isMaterial,
    isScheme,
    IconButton,
    Input,
    Sidebar,
    SidebarItem,
    SidebarSection,
    SidebarState,
    TitleBar,
    ToastRegion,
    Toaster,
    provideToaster,
    type Accent,
    type CaptionAction,
    type IconComponent,
    type Material,
    type Scheme,
    type WindowControls,
  } from '@wirecatch/ui';
  import AccentsSection from './AccentsSection.svelte';
  import AppearanceBar from './AppearanceBar.svelte';
  import ControlsSection from './ControlsSection.svelte';
  import FeedbackSection from './FeedbackSection.svelte';
  import FoundationsSection from './FoundationsSection.svelte';
  import OverlaysSection from './OverlaysSection.svelte';
  import { SYSTEM_ACCENT_SAMPLE } from './sample';

  const SECTIONS: { id: string; label: string; icon: IconComponent }[] = [
    { id: 'foundations', label: 'Foundations', icon: Palette },
    { id: 'accents', label: 'Accents and schemes', icon: Sparkles },
    { id: 'controls', label: 'Controls', icon: MousePointerClick },
    { id: 'feedback', label: 'Feedback', icon: Blocks },
    { id: 'overlays', label: 'Layers', icon: Layers },
  ];

  const toaster = new Toaster();
  provideToaster(toaster);
  const sidebar = new SidebarState();
  // The address can set the look (`?scheme=dark&accent=amber&material=mica`), so a review can link to one.
  const query = new URLSearchParams(window.location.search);
  const asked = {
    scheme: query.get('scheme'),
    accent: query.get('accent'),
    material: query.get('material'),
  };
  const systemScheme = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  let scheme = $state<Scheme>(isScheme(asked.scheme) ? asked.scheme : systemScheme);
  let accent = $state<Accent>(isAccent(asked.accent) ? asked.accent : DEFAULT_ACCENT);
  let material = $state<Material | null>(isMaterial(asked.material) ? asked.material : null);
  let current = $state('foundations');
  let search = $state('');

  $effect(() => {
    applyAppearance(document.documentElement, {
      scheme,
      accent,
      material,
      systemAccent: SYSTEM_ACCENT_SAMPLE,
    });
  });

  // Window calls only happen in the app; here they say so.
  const controls: WindowControls = {
    minimize: () => {
      toaster.info('Minimizes the window in the app.');
      return Promise.resolve();
    },
    toggleMaximize: () => {
      toaster.info('Maximizes or restores the window in the app.');
      return Promise.resolve();
    },
    close: () => Promise.reject(new Error('The gallery cannot close its browser tab.')),
    isMaximized: () => Promise.resolve(false),
    onResized: () => Promise.resolve(() => undefined),
  };

  function go(id: string) {
    current = id;
    document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }
</script>

<div class="flex h-full flex-col">
  <TitleBar
    title="Wirecatch component gallery"
    {controls}
    onerror={(_action: CaptionAction, error: unknown) =>
      toaster.danger(error instanceof Error ? error.message : 'The window call failed.')}
  >
    {#snippet leading()}
      <IconButton
        icon={PanelLeft}
        label={sidebar.showLabels ? 'Collapse navigation' : 'Expand navigation'}
        aria-expanded={sidebar.showLabels}
        onclick={() => sidebar.toggle()}
      />
    {/snippet}
    {#snippet icon()}
      <span class="size-icon-md rounded-xs bg-accent" aria-hidden="true"></span>
    {/snippet}
    {#snippet center()}
      <Input
        value={search}
        icon={Search}
        size="sm"
        placeholder="Search"
        label="Search"
        onchange={(value: string) => (search = value)}
      />
    {/snippet}
    {#snippet trailing()}
      <IconButton
        icon={scheme === 'dark' ? Sun : Moon}
        label={scheme === 'dark' ? 'Use the light scheme' : 'Use the dark scheme'}
        onclick={() => (scheme = scheme === 'dark' ? 'light' : 'dark')}
      />
    {/snippet}
  </TitleBar>

  <div class="relative flex min-h-0 flex-1">
    <Sidebar {sidebar} label="Gallery">
      <SidebarSection title="Design system">
        {#each SECTIONS as section (section.id)}
          <SidebarItem
            label={section.label}
            icon={section.icon}
            active={current === section.id}
            onclick={() => go(section.id)}
          />
        {/each}
      </SidebarSection>
    </Sidebar>
    <main
      class="relative min-w-0 flex-1 overflow-y-auto rounded-tl-lg border-t border-l border-line bg-layer"
    >
      <AppearanceBar
        {scheme}
        {accent}
        {material}
        systemAccent={SYSTEM_ACCENT_SAMPLE}
        onscheme={(next: Scheme) => (scheme = next)}
        onaccent={(next: Accent) => (accent = next)}
        onmaterial={(next: Material | null) => (material = next)}
      />
      <div class="mx-auto flex max-w-page flex-col gap-14 px-8 pt-6 pb-24">
        <FoundationsSection />
        <AccentsSection />
        <ControlsSection {toaster} />
        <FeedbackSection />
        <OverlaysSection {toaster} />
      </div>
    </main>
  </div>
</div>

<ToastRegion {toaster} />
