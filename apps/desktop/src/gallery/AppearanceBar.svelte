<script lang="ts">
  // The gallery's own appearance switches: scheme, accent and material. In the app these are settings (the theme unit's); here they write the same attributes on <html>.
  import Monitor from '~icons/lucide/monitor';
  import Moon from '~icons/lucide/moon';
  import Sun from '~icons/lucide/sun';
  import {
    ACCENTS,
    FOCUS_RING,
    Icon,
    SegmentedControl,
    Select,
    cn,
    tooltip,
    type Accent,
    type Material,
    type Scheme,
  } from '@wirecatch/ui';

  interface Props {
    scheme: Scheme;
    accent: Accent;
    material: Material | null;
    /** What the OS would report as its accent colour, for the system swatch. */
    systemAccent: string;
    onscheme: (scheme: Scheme) => void;
    onaccent: (accent: Accent) => void;
    onmaterial: (material: Material | null) => void;
  }

  let { scheme, accent, material, systemAccent, onscheme, onaccent, onmaterial }: Props = $props();

  const MATERIALS = [
    { value: 'none', label: 'Solid (Linux)' },
    { value: 'mica', label: 'Mica (Windows 11)' },
    { value: 'acrylic', label: 'Acrylic (Windows 10)' },
  ] as const;

  const accentName = (name: Accent) =>
    name === 'system' ? 'System accent' : `${name.charAt(0).toUpperCase()}${name.slice(1)}`;
</script>

<div
  class="sticky top-0 z-sticky flex flex-wrap items-center gap-x-6 gap-y-3 border-b border-line bg-overlay px-8 py-3 glass"
>
  <SegmentedControl
    label="Scheme"
    value={scheme}
    size="sm"
    options={[
      { value: 'light', label: 'Light', icon: Sun },
      { value: 'dark', label: 'Dark', icon: Moon },
    ]}
    onchange={(value: Scheme) => {
      onscheme(value);
    }}
  />

  <div role="group" aria-label="Accent" class="flex items-center gap-1.5">
    {#each ACCENTS as name (name)}
      <!-- Each swatch carries its own accent, so it shows that accent whatever the page's is. -->
      <button
        type="button"
        aria-pressed={accent === name}
        aria-label={accentName(name)}
        data-accent={name}
        style:--accent-system={name === 'system' ? systemAccent : undefined}
        class={cn(
          'flex size-6 items-center justify-center rounded-full border-2 transition',
          accent === name ? 'border-ink' : 'border-transparent hover:border-line-strong',
          FOCUS_RING,
        )}
        {@attach tooltip(() => accentName(name))}
        onclick={() => {
          onaccent(name);
        }}
      >
        <span class="flex size-4 items-center justify-center rounded-full bg-accent text-accent-fg">
          {#if name === 'system'}<Icon icon={Monitor} size="xs" />{/if}
        </span>
      </button>
    {/each}
  </div>

  <div class="ml-auto w-56">
    <Select
      label="Window material"
      size="sm"
      value={material ?? 'none'}
      options={MATERIALS}
      onchange={(value: Material | 'none') => {
        onmaterial(value === 'none' ? null : value);
      }}
    />
  </div>
</div>
