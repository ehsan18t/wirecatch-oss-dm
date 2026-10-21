<script lang="ts">
  import GallerySection from './GallerySection.svelte';
  import Specimen from './Specimen.svelte';

  // Literal class names, so Tailwind sees each one in the source.
  const SURFACES = [
    { name: 'base', note: 'The window, solid', class: 'bg-base' },
    { name: 'layer', note: 'The content', class: 'bg-layer' },
    { name: 'raised', note: 'Cards and rows', class: 'bg-raised' },
    { name: 'overlay', note: 'Menus, dialogs, toasts', class: 'bg-overlay' },
    { name: 'control', note: 'Buttons', class: 'bg-control' },
    { name: 'field', note: 'Text fields', class: 'bg-field' },
    { name: 'wash', note: 'Hover', class: 'bg-wash' },
    { name: 'wash-strong', note: 'Pressed, selected', class: 'bg-wash-strong' },
  ];

  const INKS = [
    { name: 'ink', class: 'text-ink' },
    { name: 'ink-muted', class: 'text-ink-muted' },
    { name: 'ink-subtle', class: 'text-ink-subtle' },
    { name: 'accent', class: 'text-accent' },
    { name: 'success', class: 'text-success' },
    { name: 'warning', class: 'text-warning' },
    { name: 'danger', class: 'text-danger' },
    { name: 'info', class: 'text-info' },
  ];

  const FILLS = [
    { name: 'accent', class: 'bg-accent text-accent-fg' },
    { name: 'accent-hover', class: 'bg-accent-hover text-accent-fg' },
    { name: 'accent-pressed', class: 'bg-accent-pressed text-accent-fg' },
    { name: 'accent-soft', class: 'bg-accent-soft text-accent' },
    { name: 'success', class: 'bg-success text-on-fill' },
    { name: 'warning', class: 'bg-warning text-on-fill' },
    { name: 'danger', class: 'bg-danger text-on-fill' },
    { name: 'info', class: 'bg-info text-on-fill' },
    { name: 'progress-done', class: 'bg-progress-done text-on-fill' },
  ];

  const TYPE = [
    { name: 'hero', class: 'font-display text-hero font-semibold tracking-display' },
    { name: 'title', class: 'font-display text-title font-semibold tracking-display' },
    { name: 'headline', class: 'font-display text-headline font-semibold' },
    { name: 'heading', class: 'font-display text-heading font-semibold' },
    { name: 'subtitle', class: 'text-subtitle font-semibold' },
    { name: 'lead', class: 'text-lead' },
    { name: 'body', class: 'text-body' },
    { name: 'ui', class: 'text-ui' },
    { name: 'small', class: 'text-small' },
    { name: 'caption', class: 'text-caption font-semibold tracking-caps uppercase' },
    { name: 'mono', class: 'font-mono text-small' },
  ];

  const RADII = [
    { name: 'xs', class: 'rounded-xs' },
    { name: 'sm', class: 'rounded-sm' },
    { name: 'md', class: 'rounded-md' },
    { name: 'lg', class: 'rounded-lg' },
    { name: 'xl', class: 'rounded-xl' },
  ];

  const DEPTH = [
    { name: 'control', class: 'shadow-control bg-control' },
    { name: 'raised', class: 'shadow-raised bg-raised' },
    { name: 'flyout', class: 'shadow-flyout bg-overlay' },
    { name: 'dialog', class: 'shadow-dialog bg-overlay' },
  ];
</script>

<GallerySection
  id="foundations"
  title="Foundations"
  description="Graphite on cool paper, one accent for what moves and where you are, and layers that read as layers. Every value comes from tokens.css; the components use nothing else."
>
  <Specimen title="Surfaces" note="Layers over the window base; what floats is solid">
    <div class="grid w-full grid-cols-2 gap-3 sm:grid-cols-4">
      {#each SURFACES as surface (surface.name)}
        <div class="flex flex-col gap-1.5">
          <div class="h-16 rounded-md border border-line {surface.class}"></div>
          <span class="text-ui font-medium text-ink">{surface.name}</span>
          <span class="text-small text-ink-muted">{surface.note}</span>
        </div>
      {/each}
    </div>
  </Specimen>

  <div class="grid gap-4 md:grid-cols-2">
    <Specimen title="Text" note="Every pair passes WCAG AA">
      <div class="flex w-full flex-col gap-1.5">
        {#each INKS as ink (ink.name)}
          <p class="text-body {ink.class}">{ink.name}: the download finished in 2 minutes.</p>
        {/each}
      </div>
    </Specimen>
    <Specimen title="Fills">
      <div class="grid w-full grid-cols-3 gap-2">
        {#each FILLS as fill (fill.name)}
          <div
            class="flex h-12 items-end rounded-sm px-2 pb-1.5 text-caption font-semibold {fill.class}"
          >
            {fill.name}
          </div>
        {/each}
      </div>
    </Specimen>
  </div>

  <Specimen title="Type" note="Segoe UI Variable on Windows, the system sans elsewhere">
    <div class="flex w-full flex-col gap-2">
      {#each TYPE as type (type.name)}
        <div class="flex items-baseline gap-4">
          <span class="w-20 shrink-0 text-small text-ink-subtle">{type.name}</span>
          <span class="truncate text-ink {type.class}">Catch every download</span>
        </div>
      {/each}
    </div>
  </Specimen>

  <div class="grid gap-4 md:grid-cols-2">
    <Specimen title="Radii">
      {#each RADII as radius (radius.name)}
        <div class="flex flex-col items-center gap-1.5">
          <div class="size-14 border border-line-strong bg-accent-soft {radius.class}"></div>
          <span class="text-small text-ink-muted">{radius.name}</span>
        </div>
      {/each}
    </Specimen>
    <Specimen title="Depth" note="Shadows grow with the layer">
      {#each DEPTH as depth (depth.name)}
        <div class="flex flex-col items-center gap-1.5">
          <div class="size-14 rounded-md border border-line {depth.class}"></div>
          <span class="text-small text-ink-muted">{depth.name}</span>
        </div>
      {/each}
    </Specimen>
  </div>

  <Specimen title="Scrim" note="The one backdrop blur: the veil behind a dialog (D46)">
    <div class="relative h-36 w-full overflow-hidden rounded-md border border-line">
      <div class="absolute inset-0 grid grid-cols-6" aria-hidden="true">
        <div class="bg-accent"></div>
        <div class="bg-success"></div>
        <div class="bg-warning"></div>
        <div class="bg-danger"></div>
        <div class="bg-info"></div>
        <div class="bg-ink"></div>
      </div>
      <div class="absolute inset-0 bg-scrim backdrop-blur-scrim"></div>
      <div
        class="absolute inset-x-8 inset-y-6 flex items-center justify-center rounded-lg border border-line bg-overlay text-body font-medium text-ink shadow-dialog"
      >
        A dialog over the blurred content
      </div>
    </div>
  </Specimen>
</GallerySection>
