<script lang="ts" module>
  import type { LinkProtocol, NameSyntax } from '@wirecatch/protocol';
  import type { SelectOption } from '@wirecatch/ui';

  const TAKES: readonly SelectOption<'manual' | 'auto'>[] = [
    { value: 'manual', label: 'Manual' },
    { value: 'auto', label: 'Auto' },
  ];

  const SYNTAX: readonly SelectOption<NameSyntax>[] = [
    { value: 'wildcards', label: 'Wildcards' },
    { value: 'regex', label: 'Regular expression' },
  ];

  const PROTOCOLS: readonly { value: LinkProtocol; label: string }[] = [
    { value: 'http', label: 'HTTP' },
    { value: 'https', label: 'HTTPS' },
    { value: 'ftp', label: 'FTP' },
  ];
</script>

<script lang="ts">
  // The queue editor's "Takes new downloads" card: Manual, or Auto with its switch, its claims of each kind and Try a link, which tries the claims as the form has them (D72.2, D72.7).
  import type { DraftClaims, QueueClaims, QueueId } from '@wirecatch/protocol';
  import {
    Field,
    SegmentedControl,
    Switch,
    TagInput,
    TextArea,
    ToggleChip,
    type FieldControl,
  } from '@wirecatch/ui';
  import { CATEGORIES, CATEGORY_LABEL } from '$lib/format';
  import { fileTypeClaim, takesOf, toggled } from '$lib/queue-claims';
  import ChoiceCard from './ChoiceCard.svelte';
  import TryLink from './TryLink.svelte';

  let {
    queue,
    name,
    auto,
    claims,
    onchange,
  }: {
    /** The queue edited, `null` for one not made yet. */
    queue: QueueId | null;
    /** Its name as the form has it. */
    name: string;
    auto: boolean;
    claims: QueueClaims;
    onchange: (auto: boolean, claims: QueueClaims) => void;
  } = $props();

  // The text as typed, blank lines included; the core drops those when the queue is saved.
  let fileNames = $state((() => claims.file_names.join('\n'))());

  const change = (patch: Partial<QueueClaims>) => onchange(auto, { ...claims, ...patch });
  const draft = (): DraftClaims => ({
    queue,
    takes: takesOf(auto, $state.snapshot(claims)),
  });
</script>

<div class="flex flex-col divide-y divide-line rounded-md border border-line bg-layer">
  <ChoiceCard
    flush
    label="Takes new downloads"
    description={auto
      ? 'The downloads you put in it, and new ones its claims match.'
      : 'Only the downloads you put in it.'}
    value={auto ? 'auto' : 'manual'}
    options={TAKES}
    onchange={(next: 'manual' | 'auto') => onchange(next === 'auto', claims)}
  />

  {#if auto}
    <div class="p-3">
      <Switch
        labelSide="start"
        label="Claim new downloads"
        description={claims.enabled
          ? 'New downloads its claims match come here.'
          : 'Its claims are kept, but it takes nothing new until you turn this on.'}
        checked={claims.enabled}
        onchange={(on: boolean) => change({ enabled: on })}
      />
    </div>

    <div class="flex flex-col gap-4 p-3">
      <Field
        label="Sites"
        description="A name covers the names under it. *.example.com, 10.0.0.0/8 and <local> work too."
      >
        {#snippet children(control: FieldControl)}
          <TagInput
            {...control}
            label="Sites"
            values={claims.sites}
            placeholder="youtube.com"
            onchange={(sites: string[]) => change({ sites })}
          />
        {/snippet}
      </Field>

      <div class="flex flex-col gap-2">
        <Field
          label="File names"
          description={claims.file_name_syntax === 'wildcards'
            ? 'One per line. * stands for any text and ? for one character; case is ignored.'
            : 'One regular expression per line; case is ignored.'}
        >
          {#snippet children(control: FieldControl)}
            <TextArea
              {...control}
              mono
              value={fileNames}
              placeholder={claims.file_name_syntax === 'wildcards'
                ? '*.iso'
                : String.raw`^backup-\d+`}
              onchange={(text: string) => {
                fileNames = text;
                change({ file_names: text.split('\n') });
              }}
            />
          {/snippet}
        </Field>
        <SegmentedControl
          label="File names are written as"
          class="self-start"
          size="sm"
          value={claims.file_name_syntax}
          options={SYNTAX}
          onchange={(syntax: NameSyntax) => change({ file_name_syntax: syntax })}
        />
      </div>

      <Field label="File types" description="Extensions, with or without the dot.">
        {#snippet children(control: FieldControl)}
          <TagInput
            {...control}
            label="File types"
            values={claims.file_types}
            normalize={fileTypeClaim}
            placeholder="iso, tar.gz"
            onchange={(file_types: string[]) => change({ file_types })}
          />
        {/snippet}
      </Field>

      <div class="flex flex-col gap-2">
        <span class="text-ui font-medium text-ink" aria-hidden="true">Categories</span>
        <div class="flex flex-wrap gap-1" role="group" aria-label="Categories">
          {#each CATEGORIES as category (category)}
            <ToggleChip
              pressed={claims.categories.includes(category)}
              label={CATEGORY_LABEL[category]}
              onchange={(on: boolean) =>
                change({ categories: toggled(claims.categories, category, on) })}
            />
          {/each}
        </div>
      </div>

      <div class="flex flex-col gap-2">
        <span class="text-ui font-medium text-ink" aria-hidden="true">Protocols</span>
        <div class="flex flex-wrap gap-1" role="group" aria-label="Protocols">
          {#each PROTOCOLS as protocol (protocol.value)}
            <ToggleChip
              pressed={claims.protocols.includes(protocol.value)}
              label={protocol.label}
              onchange={(on: boolean) =>
                change({ protocols: toggled(claims.protocols, protocol.value, on) })}
            />
          {/each}
        </div>
      </div>

      <p class="text-small text-ink-muted">
        A new download goes to the first claim that matches it: site, then file name, file type,
        category and protocol. When two queues match it the same way, the one higher in the sidebar
        takes it.
      </p>
    </div>
  {/if}

  <div class="p-3">
    <TryLink {draft} draftName={name} />
  </div>
</div>
