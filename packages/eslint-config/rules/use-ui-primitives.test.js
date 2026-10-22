import { after, describe, it } from 'node:test';
import { RuleTester } from 'eslint';
import svelte from 'eslint-plugin-svelte';
import ts from 'typescript-eslint';
import { useUiPrimitives } from './use-ui-primitives.js';

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.afterAll = after;

const svelteParser = svelte.configs.base.find((c) => c.languageOptions?.parser)?.languageOptions
  .parser;
const tester = new RuleTester({
  languageOptions: { parser: svelteParser, parserOptions: { parser: ts.parser } },
});
const options = [{ buttonClasses: { btn: 'Button', 'icon-btn': 'IconButton' }, icon: 'Icon' }];
const filename = 'Example.svelte';
const script = (body) => `<script lang="ts">\n${body}\n</script>\n`;
const valid = (code) => ({ code, options, filename });
const primitiveClass = (code) => ({
  code,
  options,
  filename,
  errors: [{ messageId: 'primitiveClass' }],
});
const directIcon = (code) => ({ code, options, filename, errors: [{ messageId: 'directIcon' }] });
/** Options adding a module icons come from, as a package with icons of its own would. */
const ownIcons = [{ ...options[0], extraIconSources: ['@wirecatch/ui/icons'] }];

tester.run('use-ui-primitives', useUiPrimitives, {
  valid: [
    // Buttons with styles of their own stay plain buttons.
    valid('<button type="button" class="item">Downloads</button>'),
    valid('<button type="button" role="menuitem" class={["entry", on && "on"]}>Open</button>'),
    // A class that only starts like a primitive's is a different class.
    valid('<button type="button" class="btn-row">Row</button>'),
    // A string that is compared, not used as a class.
    valid('<button type="button" class={kind === "btn" ? "a" : "b"}>Row</button>'),
    valid('<div class={{ active: kind === "icon-btn" }}></div>'),
    // The primitives themselves, and icons drawn through Icon.
    valid('<Button variant="primary">Save</Button>'),
    valid(
      `${script("import X from '@lucide/svelte/icons/x';\nimport Icon from './Icon.svelte';")}<Icon icon={X} size="sm" />`,
    ),
    // Another component's size is its own business.
    valid(`${script("import Dialog from './Dialog.svelte';")}<Dialog size="wide" />`),
    // A type import cannot be rendered.
    valid(`${script("import type { LucideIcon } from '@lucide/svelte';")}<LucideIcon />`),
    // A name that holds an icon, passed to Icon, and a dynamic component that is not an icon.
    valid(
      `${script("import X from '@lucide/svelte/icons/x';\nimport Icon from './Icon.svelte';\nconst Close = X;")}<Icon icon={Close} size="sm" />`,
    ),
    valid(
      `${script("import Dialog from './Dialog.svelte';\nconst Shown = Dialog;")}<Shown /><svelte:component this={Dialog} />`,
    ),
    // A name declared from itself does not loop.
    valid(`${script('let Loop = Loop;')}<Loop />`),
    // A name that shadows an icon is another value.
    valid(
      `${script("import X from '@lucide/svelte/icons/x';\nimport Icon from './Icon.svelte';\nimport Dialog from './Dialog.svelte';")}<Icon icon={X} />{#each [Dialog] as X}<X />{/each}`,
    ),
    valid(
      `${script("import Dialog from './Dialog.svelte';\nfunction pick() { const Close = Dialog; return Close; }\nimport X from '@lucide/svelte/icons/x';\nconst Close = Dialog;\nvoid X; void pick;")}<Close />`,
    ),
    // Icons drawn through Icon, from unplugin-icons and from a module the options name.
    valid(
      `${script("import X from '~icons/lucide/x';\nimport Icon from './Icon.svelte';")}<Icon icon={X} size="sm" />`,
    ),
    // A module that only starts like an icon source is another module.
    valid(`${script("import Panel from '~iconsets/panel';")}<Panel />`),
    valid(`${script("import Badge from '@lucide/svelte-extras/badge';")}<Badge />`),
    // With its own icon module added, a package's other modules are still not icons.
    {
      code: `${script("import Panel from '@wirecatch/ui/panel';")}<Panel />`,
      options: ownIcons,
      filename,
    },
    // A class name from a name that holds another class.
    valid(`${script("const cls = 'btn-row';")}<button type="button" class={cls}>Row</button>`),
    // A name destructured from a const is its own property, not the whole object.
    valid(
      `${script("import X from '@lucide/svelte/icons/x';\nimport Icon from './Icon.svelte';\nimport EmptyState from './EmptyState.svelte';\nconst { view: V, icon } = { view: EmptyState, icon: X };")}<Icon {icon} /><V />`,
    ),
    // An {#each} item destructured to one property is that property, not the whole element.
    valid(
      `${script("import X from '@lucide/svelte/icons/x';\nimport Icon from './Icon.svelte';\nimport Panel from './Panel.svelte';\nconst items = [{ icon: X, view: Panel }];")}{#each items as { icon, view: V } (icon)}<Icon {icon} /><V />{/each}`,
    ),
    // A member reads its own property.
    valid(
      `${script("const C = { primary: 'primary', icon: 'icon-btn' };")}<button type="button" class={C.primary}>Save</button>`,
    ),
  ],
  invalid: [
    primitiveClass('<button type="button" class="btn primary">Save</button>'),
    primitiveClass(
      '<button type="button" class={["icon-btn", extra]} aria-label="Close">x</button>',
    ),
    primitiveClass('<button type="button" class:btn={true}>Save</button>'),
    primitiveClass('<button type="button" class="{small ? `btn small` : `btn`}">Save</button>'),
    primitiveClass('<button type="button" class={{ btn: true, primary }}>Save</button>'),
    primitiveClass('<button type="button" class={wide || "btn"}>Save</button>'),
    // On any element, not only a button.
    primitiveClass('<a class="btn" href="/help">Help</a>'),
    directIcon(`${script("import X from '@lucide/svelte/icons/x';")}<X size={14} />`),
    directIcon(`${script("import { Pause } from '@lucide/svelte';")}<Pause aria-hidden="true" />`),
    directIcon(`${script("import X from '@lucide/svelte/icons/x';")}<X strokeWidth={1.5} />`),
    directIcon(`${script("import * as L from '@lucide/svelte';")}<L.X size={14} />`),
    // Icons unplugin-icons serves, and those of a module the options name.
    directIcon(`${script("import X from '~icons/lucide/x';")}<X class="size-4" />`),
    directIcon(`${script("import X from '~icons/lucide/x';")}<svelte:component this={X} />`),
    directIcon(`${script("import X from 'virtual:icons/lucide/x';")}<X />`),
    directIcon(`${script("import X from 'virtual/icons/lucide/x';")}<X />`),
    // Adding a package's own icon module keeps Lucide and unplugin-icons counting as icons.
    {
      code: `${script("import X from '@lucide/svelte/icons/x';")}<X />`,
      options: ownIcons,
      filename,
      errors: [{ messageId: 'directIcon' }],
    },
    {
      code: `${script("import X from '~icons/lucide/x';")}<X />`,
      options: ownIcons,
      filename,
      errors: [{ messageId: 'directIcon' }],
    },
    {
      code: `${script("import X from '@wirecatch/ui/icons/x';")}<X />`,
      options: ownIcons,
      filename,
      errors: [{ messageId: 'directIcon' }],
    },
    {
      code: `${script("import { X } from '@wirecatch/ui/icons';")}<X />`,
      options: ownIcons,
      filename,
      errors: [{ messageId: 'directIcon' }],
    },
    // The markup may come before the script.
    directIcon(`<X />\n${script("import X from '@lucide/svelte/icons/x';")}`),
    // Through a name it was given, in the script or the markup.
    directIcon(`${script("import X from '@lucide/svelte/icons/x';\nconst Close = X;")}<Close />`),
    directIcon(
      `${script("import X from '@lucide/svelte/icons/x';\nconst First = X;\nconst Close = First;")}<Close />`,
    ),
    directIcon(
      `${script("import { Pause, Play } from '@lucide/svelte';")}{#if on}{@const Shown = paused ? Play : Pause}<Shown />{/if}`,
    ),
    directIcon(
      `${script("import X from '@lucide/svelte/icons/x';\nconst icons = { close: X };")}<icons.close />`,
    ),
    // Through svelte:component.
    directIcon(`${script("import X from '@lucide/svelte/icons/x';")}<svelte:component this={X} />`),
    directIcon(
      `${script("import * as L from '@lucide/svelte';\nconst Close = L.X;")}<svelte:component this={Close} size={14} />`,
    ),
    // Through a rune, and through an {#each} over icons, written in place or named.
    directIcon(
      `${script("import X from '@lucide/svelte/icons/x';\nimport Check from '@lucide/svelte/icons/check';\nconst D = $derived(on ? X : Check);")}<D />`,
    ),
    directIcon(
      `${script("import X from '@lucide/svelte/icons/x';")}{#each [X] as I (I)}<I />{/each}`,
    ),
    directIcon(
      `${script("import X from '@lucide/svelte/icons/x';\nconst icons = [X];")}{#each icons as I (I)}<I />{/each}`,
    ),
    // A primitive's class through a name.
    primitiveClass(
      `${script("const cls = 'btn';")}<button type="button" class={cls}>Save</button>`,
    ),
    primitiveClass(
      `${script("const base = 'icon-btn';\nconst cls = $derived(small ? base : 'x');")}<button type="button" class={[cls]}>x</button>`,
    ),
    // A destructuring not followed property by property stands for the whole value.
    directIcon(`${script("import X from '@lucide/svelte/icons/x';\nconst [C] = [X];")}<C />`),
    directIcon(
      `${script("import X from '@lucide/svelte/icons/x';\nconst { ...r } = { c: X };")}<r.c />`,
    ),
    // Both branches of a choice are followed, whichever comes first.
    primitiveClass(
      `${script("const S = { plain: 'plain', primary: 'btn' };")}<button type="button" class={on ? S.plain : S.primary}>Save</button>`,
    ),
    primitiveClass(
      `${script("const S = { plain: 'plain', primary: 'btn' };")}<button type="button" class={on ? S.primary : S.plain}>Save</button>`,
    ),
    directIcon(
      `${script("import X from '@lucide/svelte/icons/x';\nimport Panel from './Panel.svelte';\nconst I = { a: Panel, b: X };")}<svelte:component this={on ? I.a : I.b} />`,
    ),
    directIcon(
      `${script("import X from '@lucide/svelte/icons/x';\nimport Panel from './Panel.svelte';\nconst I = { a: Panel, b: X };")}<svelte:component this={on ? I.b : I.a} />`,
    ),
    // Through the other runes, a member, and the matching property of an {#each} item.
    directIcon(
      `${script("import X from '@lucide/svelte/icons/x';\nconst D = $derived.by(() => { if (on) return X; return null; });")}<D />`,
    ),
    directIcon(`${script("import X from '@lucide/svelte/icons/x';\nlet S = $state.raw(X);")}<S />`),
    directIcon(
      `${script("import X from '@lucide/svelte/icons/x';\nconst items = [{ icon: X }];")}{#each items as { icon: I } (I)}<I />{/each}`,
    ),
    primitiveClass(
      `${script("const C = { primary: 'btn' };")}<button type="button" class={C.primary}>Save</button>`,
    ),
  ],
});

/** Options that also report native form controls, as the desktop app's and the extension's do. */
const withControls = [{ ...options[0], controls: true }];
const nativeControl = (code) => ({
  code,
  options: withControls,
  filename,
  errors: [{ messageId: 'nativeControl' }],
});

tester.run('use-ui-primitives, native controls', useUiPrimitives, {
  valid: [
    // A text field with a role of its own, the design system's controls, and the option left off.
    { code: '<input role="combobox" aria-expanded="true" />', options: withControls, filename },
    { code: '<input type="search" />', options: withControls, filename },
    {
      code: '<Select value={v} options={o} /><Checkbox checked={on} />',
      options: withControls,
      filename,
    },
    valid('<select><option>a</option></select>'),
  ],
  invalid: [
    nativeControl('<select><option>a</option></select>'),
    nativeControl('<textarea></textarea>'),
    nativeControl('<input type="checkbox" />'),
    nativeControl('<input type="time" bind:value={t} />'),
    nativeControl('<input class="field" type="number" min="1" />'),
    nativeControl('<input type="range" />'),
    nativeControl('<input type="Checkbox" />'),
  ],
});
