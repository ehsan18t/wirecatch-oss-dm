<script lang="ts" generics="T extends string | number">
  // One of the queue dialog's cards, or a row of a grouped card (`flush`): a choice between a few options, with what the one chosen means under its name.
  import { SegmentedControl, type SelectOption } from '@wirecatch/ui';

  let {
    label,
    description,
    value,
    options,
    onchange,
    flush = false,
  }: {
    label: string;
    description: string;
    value: T;
    options: readonly SelectOption<T>[];
    onchange: (value: T) => void;
    flush?: boolean;
  } = $props();

  const id = $props.id();
</script>

<section
  class={[
    'flex flex-wrap items-center justify-between gap-3 p-3',
    !flush && 'rounded-md border border-line bg-layer',
  ]}
  aria-labelledby="{id}-label"
  aria-describedby="{id}-help"
>
  <div class="flex min-w-0 flex-1 basis-56 flex-col gap-0.5">
    <span id="{id}-label" class="text-ui font-medium text-ink">{label}</span>
    <span id="{id}-help" class="text-small text-ink-muted">{description}</span>
  </div>
  <SegmentedControl {label} size="sm" {value} {options} {onchange} />
</section>
