<script lang="ts" module>
  import type { LinkProtocol, NameSyntax, QueueRules } from '@wirecatch/protocol';
  import type { SelectOption } from '@wirecatch/ui';

  const SYNTAX: readonly SelectOption<NameSyntax>[] = [
    { value: 'wildcards', label: 'Wildcards' },
    { value: 'regex', label: 'Regular expression' },
  ];

  const PROTOCOLS: readonly { value: LinkProtocol; label: string }[] = [
    { value: 'http', label: 'HTTP' },
    { value: 'https', label: 'HTTPS' },
    { value: 'ftp', label: 'FTP and FTPS' },
  ];

  type Kind = 'sites' | 'file_names' | 'file_types' | 'categories' | 'protocols';

  const AUTO_HELP =
    'New downloads that match any rule below go to this queue. Rules are checked in this order: site, file name, file type, category, protocol; if several queues match, the one highest in the sidebar wins.';
</script>

<script lang="ts">
  // The queue editor's rules: one switch, and each kind of rule behind its own checkbox, its values kept while unchecked until the queue is saved (D84.4, D84.5). Test a link tries the rules as the form has them (D72.7).
  import type { DraftRules, QueueId } from '@wirecatch/protocol';
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
  import { fileTypeRule, hasRules, addDownloadsOf, toggled } from '$lib/queue-rules';
  import RuleToggle from './RuleToggle.svelte';
  import TryLink from './TryLink.svelte';

  let {
    queue,
    name,
    auto,
    rules,
    onchange,
  }: {
    /** The queue edited, `null` for one not made yet. */
    queue: QueueId | null;
    /** Its name as the form has it. */
    name: string;
    auto: boolean;
    rules: QueueRules;
    onchange: (auto: boolean, rules: QueueRules) => void;
  } = $props();

  const on = $derived(auto && rules.enabled);
  // The text as typed, blank lines included; the core drops those when the queue is saved.
  let fileNames = $state((() => rules.file_names.join('\n'))());
  // An unchecked kind's values, back if it is checked again before the queue is saved.
  const stash: Partial<Pick<QueueRules, Kind>> = {};
  const checked = $state<Record<Kind, boolean>>(
    (() => ({
      sites: rules.sites.length > 0,
      file_names: rules.file_names.some((n) => n.trim() !== ''),
      file_types: rules.file_types.length > 0,
      categories: rules.categories.length > 0,
      protocols: rules.protocols.length > 0,
    }))(),
  );

  const change = (patch: Partial<QueueRules>) => onchange(auto, { ...rules, ...patch });
  const draft = (): DraftRules => ({
    queue,
    add_downloads: addDownloadsOf(auto, $state.snapshot(rules)),
  });

  function switchAuto(next: boolean) {
    // Off keeps the rules for later, as "Auto off" (D72.4); a queue with none is simply manual.
    if (next) onchange(true, { ...rules, enabled: true });
    else onchange(hasRules(rules), { ...rules, enabled: false });
  }

  function check(kind: Kind, next: boolean) {
    checked[kind] = next;
    if (next) {
      const kept = stash[kind];
      Object.assign(stash, { [kind]: undefined });
      if (kept !== undefined) change({ [kind]: kept });
    } else {
      Object.assign(stash, { [kind]: $state.snapshot(rules[kind]) });
      change({ [kind]: [] });
    }
  }
</script>

<div class="flex flex-col divide-y divide-line rounded-md border border-line bg-layer">
  <div class="p-3">
    <Switch
      labelSide="start"
      label="Add matching downloads automatically"
      help={AUTO_HELP}
      checked={on}
      onchange={switchAuto}
    />
  </div>

  {#if on}
    <div class="flex flex-col gap-3 p-3">
      <RuleToggle
        label="By site"
        checked={checked.sites}
        onchange={(next: boolean) => {
          check('sites', next);
        }}
      >
        <Field
          label="Sites"
          hideLabel
          description="A site covers its subdomains. Wildcards (*.example.com), IP addresses, ranges (10.0.0.0/8) and <local> (names without a dot) work too."
        >
          {#snippet children(control: FieldControl)}
            <TagInput
              {...control}
              label="Sites"
              values={rules.sites}
              placeholder="youtube.com"
              onchange={(sites: string[]) => change({ sites })}
            />
          {/snippet}
        </Field>
      </RuleToggle>
      <RuleToggle
        label="By file name"
        checked={checked.file_names}
        onchange={(next: boolean) => {
          check('file_names', next);
        }}
      >
        <SegmentedControl
          label="Match file names using"
          class="self-start"
          size="sm"
          value={rules.file_name_syntax}
          options={SYNTAX}
          onchange={(syntax: NameSyntax) => change({ file_name_syntax: syntax })}
        />
        <Field
          label="File names"
          hideLabel
          description={rules.file_name_syntax === 'wildcards'
            ? 'One per line. * stands for any text and ? for one character; case is ignored.'
            : 'One regular expression per line; case is ignored.'}
        >
          {#snippet children(control: FieldControl)}
            <TextArea
              {...control}
              mono
              value={fileNames}
              placeholder={rules.file_name_syntax === 'wildcards'
                ? '*.iso'
                : String.raw`^backup-\d+`}
              onchange={(text: string) => {
                fileNames = text;
                change({ file_names: text.split('\n') });
              }}
            />
          {/snippet}
        </Field>
      </RuleToggle>
      <RuleToggle
        label="By file type"
        checked={checked.file_types}
        onchange={(next: boolean) => {
          check('file_types', next);
        }}
      >
        <Field label="File types" hideLabel description="Extensions, with or without the dot.">
          {#snippet children(control: FieldControl)}
            <TagInput
              {...control}
              label="File types"
              values={rules.file_types}
              normalize={fileTypeRule}
              placeholder="iso, tar.gz"
              onchange={(file_types: string[]) => change({ file_types })}
            />
          {/snippet}
        </Field>
      </RuleToggle>
      <RuleToggle
        label="By category"
        checked={checked.categories}
        onchange={(next: boolean) => {
          check('categories', next);
        }}
      >
        <div class="flex flex-wrap gap-1" role="group" aria-label="Categories">
          {#each CATEGORIES as category (category)}
            <ToggleChip
              pressed={rules.categories.includes(category)}
              label={CATEGORY_LABEL[category]}
              onchange={(pressed: boolean) =>
                change({ categories: toggled(rules.categories, category, pressed) })}
            />
          {/each}
        </div>
      </RuleToggle>
      <RuleToggle
        label="By protocol"
        checked={checked.protocols}
        onchange={(next: boolean) => {
          check('protocols', next);
        }}
      >
        <div class="flex flex-wrap gap-1" role="group" aria-label="Protocols">
          {#each PROTOCOLS as protocol (protocol.value)}
            <ToggleChip
              pressed={rules.protocols.includes(protocol.value)}
              label={protocol.label}
              onchange={(pressed: boolean) =>
                change({ protocols: toggled(rules.protocols, protocol.value, pressed) })}
            />
          {/each}
        </div>
      </RuleToggle>
      {#if !hasRules(rules)}
        <p class="text-small text-warning">Add a rule, or this queue adds nothing by itself.</p>
      {/if}
    </div>

    {#if hasRules(rules)}
      <div class="p-3">
        <TryLink {draft} draftName={name} />
      </div>
    {/if}
  {/if}
</div>
