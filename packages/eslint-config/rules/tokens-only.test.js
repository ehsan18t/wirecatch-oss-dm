import { after, describe, it } from 'node:test';
import { RuleTester } from 'eslint';
import svelte from 'eslint-plugin-svelte';
import ts from 'typescript-eslint';
import { tokensOnly } from './tokens-only.js';

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.afterAll = after;

const svelteParser = svelte.configs.base.find((c) => c.languageOptions?.parser)?.languageOptions
  .parser;
const tester = new RuleTester({
  languageOptions: { parser: svelteParser, parserOptions: { parser: ts.parser } },
});
const filename = 'Example.svelte';
const script = (body) => `<script lang="ts">\n${body}\n</script>\n`;
const valid = (code) => ({ code, filename });
const invalid = (code, ...messageIds) => ({
  code,
  filename,
  errors: messageIds.map((messageId) => ({ messageId })),
});

tester.run('tokens-only', tokensOnly, {
  valid: [
    // Token utilities, the spacing scale, fractions, arbitrary variants and named steps.
    valid('<div class="h-control w-sidebar p-2.5 gap-3 top-1/2 [&_svg]:size-4"></div>'),
    valid('<div class="opacity-disabled bg-danger/soft hover:bg-accent/hover"></div>'),
    valid('<div class="opacity-0 group-hover:opacity-100"></div>'),
    valid(`${script("const BUTTON = 'bg-accent text-accent-fg enabled:hover:bg-accent-hover';")}`),
    // Strings that are not classes: selectors, paths, ids, text.
    valid(
      `${script("const tabs = list.querySelectorAll('[role=\"tab\"]');\nimport X from '~icons/lucide/x';\nconst id = `#${CSS.escape(name)}`;")}`,
    ),
    valid('<p title="Downloads at 50 percent">Item 3/4</p>'),
    // Styles built from values, and a style with nothing written out.
    valid('<div style="{anchored.style} min-width: {anchored.anchorWidth}px;"></div>'),
    valid('<input style:--slider-fill="{fill}%" />'),
    // An icon's own geometry is its drawing; its size comes from a class.
    valid(
      '<svg class="size-glyph" viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" /></svg>',
    ),
    // Strings that only look like style text: accent names, a message with a colon, variants.
    valid(`${script("const ACCENTS = ['red', 'teal', 'orange'];")}`),
    valid(`${script("const message = 'Error: the folder is missing';")}`),
    valid(
      `${script("const S = 'hover:bg-wash focus-visible:outline-2 forced-colors:bg-sys-highlight';")}`,
    ),
    // Styles computed from values, and named steps through a class: directive.
    valid(`${script("element.style.setProperty('--slider-fill', `${fill}%`);")}`),
    valid('<div class:opacity-disabled={off}></div>'),
    valid('<svelte:element this="div" class="size-icon-sm"></svelte:element>'),
    // The design system's own stylesheets, and motion through its presets and tokens.
    valid(`${script("import '@wirecatch/ui/tokens.css';\nimport '@wirecatch/ui/base.css';")}`),
    valid(
      "<div transition:shift={{ from: 'right', by: 'panel' }} in:fade={{ speed: 'slow' }}></div>",
    ),
    valid(
      `${script("element.animate([{ opacity: 0 }, { opacity: 1 }], { duration: duration('normal') });")}`,
    ),
    valid('<div class="transition duration-normal delay-reveal"></div>'),
    valid(`${script('const last = items[items.length - 3];')}`),
  ],
  invalid: [
    invalid('<div class="w-[13px]"></div>', 'arbitraryValue'),
    invalid('<div class="hover:shadow-[0_0_1px_red]"></div>', 'arbitraryValue'),
    invalid('<div class="bg-(--brand) p-2"></div>', 'arbitraryValue'),
    invalid('<div class="[color:red]"></div>', 'arbitraryValue'),
    invalid(`${script("const S = cn('p-2', on && 'max-h-[80vh]');")}`, 'arbitraryValue'),
    invalid(`${script('const S = `px-2 ${on ? "w-[3px]" : ""}`;')}`, 'arbitraryValue'),
    invalid('<div class="disabled:opacity-45"></div>', 'rawOpacity'),
    invalid('<div class="bg-danger/14"></div>', 'rawAlpha'),
    invalid('<div class="active:text-white/80"></div>', 'rawAlpha'),
    invalid(`${script("const C = '#0a6b70';")}`, 'rawColor'),
    invalid(`${script("const C = 'oklch(0.7 0.1 200)';")}`, 'rawColor'),
    invalid('<div style="padding: 13px"></div>', 'rawLength'),
    invalid('<div style="color: #fff"></div>', 'rawColor'),
    invalid('<div style:width="2rem"></div>', 'rawLength'),
    invalid('<div style:margin={"4px"}></div>', 'rawLength'),
    invalid('<svg width="10" height="10" viewBox="0 0 10 10"></svg>', 'svgSize', 'svgSize'),
    // A <style> block, with raw values or through @apply.
    invalid(
      '<div class="x"></div>\n<style>\n  .x { padding: 13px; color: #f00; }\n</style>',
      'styleBlock',
    ),
    invalid(
      '<div class="x"></div>\n<style>\n  .x { @apply opacity-45 w-[13px]; }\n</style>',
      'styleBlock',
    ),
    // Named colours written into a style.
    invalid('<div style="color: red; background: white"></div>', 'rawColor'),
    // Strings in code that write styles.
    invalid(`${script("const S = 'color:#fff';")}`, 'rawColor'),
    invalid(`${script("const S = 'padding: 13px; color: tomato';")}`, 'rawLength', 'rawColor'),
    invalid(`${script("const S = '--gap: 4px';")}`, 'rawLength'),
    invalid(`${script("element.style.padding = '13px';")}`, 'rawLength'),
    invalid(`${script("element.style.color = 'red';")}`, 'rawColor'),
    invalid(`${script("element.style.setProperty('width', '200px');")}`, 'rawLength'),
    // Classes named by a class: directive.
    invalid('<div class:w-[13px]={on}></div>', 'arbitraryValue'),
    invalid('<div class:opacity-45={on}></div>', 'rawOpacity'),
    // A stylesheet beside a component, statically or dynamically.
    invalid(`${script("import './probe.css';")}`, 'cssImport'),
    invalid(`${script("const sheet = import('./theme.css?url');")}`, 'cssImport'),
    // A <style> element in the markup, such as inside <svelte:head>.
    invalid('<svelte:head><style>.x { padding: 13px; }</style></svelte:head>', 'styleBlock'),
    // Style objects given to an element.
    invalid(
      `${script("Object.assign(element.style, { padding: '13px', color: 'red' });")}`,
      'rawLength',
      'rawColor',
    ),
    invalid(
      `${script("element.animate([{ translate: '0 8px' }], { duration: 250 });")}`,
      'rawLength',
      'rawMotion',
    ),
    // Raw durations and distances in transitions and utilities.
    invalid('<div transition:fly={{ y: 13, duration: 250 }}></div>', 'rawMotion', 'rawMotion'),
    invalid('<div in:fade={{ delay: 100 }}></div>', 'rawMotion'),
    invalid('<div class="transition duration-300 delay-75"></div>', 'rawMotion'),
    // An svg rendered through svelte:element.
    invalid('<svelte:element this="svg" width="10"></svelte:element>', 'svgSize'),
  ],
});
