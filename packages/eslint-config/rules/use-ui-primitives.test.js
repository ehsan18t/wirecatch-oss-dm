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
    // The markup may come before the script.
    directIcon(`<X />\n${script("import X from '@lucide/svelte/icons/x';")}`),
  ],
});
