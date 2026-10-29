<script lang="ts" generics="T extends string | number">
  // One of the queue dialog's cards, or a row of a grouped card (`flush`): a choice between a few options, its meaning behind an info icon (D84).
  import { InfoTip, SegmentedControl, type SelectOption } from '@wirecatch/ui';

  let {
    label,
    help,
    warning = null,
    value,
    options,
    onchange,
    flush = false,
  }: {
    label: string;
    help: string;
    /** Why the choice does nothing yet, under it. */
    warning?: string | null;
    value: T;
    options: readonly SelectOption<T>[];
    onchange: (value: T) => void;
    flush?: boolean;
  } = $props();

  const id = $props.id();
</script>

<section
  class={[
    'flex flex-wrap items-center justify-between gap-x-3 gap-y-2 p-3',
    !flush && 'rounded-md border border-line bg-layer',
  ]}
  aria-labelledby="{id}-label"
  aria-describedby="{id}-help"
>
  <div class="flex min-w-0 flex-1 basis-56 items-center gap-1">
    <span id="{id}-label" class="text-ui font-medium text-ink">{label}</span>
    <InfoTip {label} text={help} />
    <span id="{id}-help" class="sr-only">{help}</span>
  </div>
  <SegmentedControl {label} size="sm" {value} {options} {onchange} />
  {#if warning}
    <p class="basis-full text-small text-warning">{warning}</p>
  {/if}
</section>
