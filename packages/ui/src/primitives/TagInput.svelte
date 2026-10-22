<script lang="ts">
  // A short list of words in one box, such as sites or file extensions: each is a chip with its own remove button, and typing adds more. Enter, a comma, a space or leaving the box adds what is typed (a paste of several adds them all); Backspace in the empty box takes the last one back to edit.
  import X from '~icons/lucide/x';
  import { cn } from '../lib/cn';
  import { field, FIELD_INPUT } from '../recipes/field';
  import IconButton from './IconButton.svelte';

  interface Props {
    values: readonly string[];
    /** The list's name, for the box and each chip's remove button. */
    label: string;
    /** The box's id, for a `<label for>` (Field and SettingRow pass it). */
    id?: string | undefined;
    'aria-describedby'?: string | undefined;
    placeholder?: string;
    /** A typed word as it is kept, or null when it is not one (it then stays in the box, marked). */
    normalize?: (typed: string) => string | null;
    invalid?: boolean;
    disabled?: boolean;
    class?: string | undefined;
    onchange: (values: string[]) => void;
  }

  let {
    values,
    label,
    id,
    'aria-describedby': describedBy,
    placeholder,
    normalize = (typed: string) => typed.trim().toLowerCase() || null,
    invalid = false,
    disabled = false,
    class: className,
    onchange,
  }: Props = $props();

  let draft = $state('');
  let refused = $state(false);
  /** What just changed, for screen readers: a chip added or taken away is not otherwise said. */
  let said = $state('');
  const uid = $props.id();
  const refusedId = `${uid}-refused`;
  let input = $state<HTMLInputElement | null>(null);

  /** Adds every word in `text`; the ones that are not words stay in the box. */
  function add(text: string) {
    const words = text.split(/[\s,;]+/).filter(Boolean);
    const kept: string[] = [];
    const left: string[] = [];
    for (const word of words) {
      const value = normalize(word);
      if (value === null) left.push(word);
      else if (!values.includes(value) && !kept.includes(value)) kept.push(value);
    }
    if (kept.length > 0) {
      onchange([...values, ...kept]);
      said = `Added ${kept.join(', ')}.`;
    }
    draft = left.join(' ');
    refused = left.length > 0;
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Enter' || event.key === ',' || (event.key === ' ' && draft.trim() !== '')) {
      event.preventDefault();
      add(draft);
    } else if (event.key === 'Backspace' && draft === '' && values.length > 0) {
      event.preventDefault();
      const last = values.at(-1) ?? '';
      onchange(values.slice(0, -1));
      draft = last;
      said = `Removed ${last}, back in the box to edit.`;
    }
  }

  function remove(value: string) {
    onchange(values.filter((v) => v !== value));
    said = `Removed ${value}.`;
    input?.focus();
  }
</script>

<!-- The text field fills what is left of the last line, so a click on the box's free space lands in it. -->
<div
  class={field({
    invalid: invalid || refused,
    multiline: true,
    class: cn('flex-wrap gap-1.5 px-1.5', className),
  })}
>
  {#if values.length > 0}
    <!-- `contents`, so the chips wrap with the field as one row of the box; the roles are written out, since some engines drop a list's role with its box. -->
    <ul class="contents" role="list" aria-label={label}>
      {#each values as value (value)}
        <li
          role="listitem"
          class="inline-flex h-6 max-w-full items-center gap-0.5 rounded-xs bg-wash-strong pr-0.5 pl-2 text-ui text-ink"
        >
          <span class="truncate">{value}</span>
          <IconButton
            icon={X}
            size="xs"
            label="Remove {value} from {label.toLowerCase()}"
            {disabled}
            onclick={() => remove(value)}
          />
        </li>
      {/each}
    </ul>
  {/if}
  <input
    bind:this={input}
    bind:value={draft}
    {id}
    {disabled}
    class={cn(FIELD_INPUT, 'h-6 min-w-24 flex-1 px-1')}
    aria-label={id === undefined ? label : undefined}
    aria-describedby={[describedBy, refused ? refusedId : null].filter(Boolean).join(' ') ||
      undefined}
    aria-invalid={invalid || refused || undefined}
    placeholder={values.length === 0 ? placeholder : undefined}
    autocomplete="off"
    spellcheck="false"
    {onkeydown}
    oninput={() => (refused = false)}
    onblur={() => {
      if (draft.trim() !== '') add(draft);
    }}
    onpaste={(event) => {
      const text = event.clipboardData?.getData('text') ?? '';
      if (!/[\s,;]/.test(text.trim())) return;
      event.preventDefault();
      add(`${draft} ${text}`);
    }}
  />
</div>
<p id={refusedId} class="sr-only" hidden={!refused}>
  “{draft}” is not one {label.toLowerCase()} takes. Edit it, or clear it.
</p>
<p class="sr-only" aria-live="polite">{said}</p>
