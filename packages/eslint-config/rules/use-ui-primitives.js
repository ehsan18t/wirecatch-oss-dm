// Keeps buttons and icons going through a package's primitives once it has them.
//
// A Svelte package that has a text button, an icon button and an icon component (the desktop app's Button, IconButton and Icon) turns this rule on everywhere except in those files. It then reports the two patterns the primitives replaced:
//
// - an element that carries one of the primitives' own classes (`btn`, `icon-btn`), which copies the primitive by hand instead of using it. Buttons with styles of their own, such as menu items and switches, stay plain buttons.
// - a Lucide icon rendered directly, with or without a size or stroke of its own, instead of through the icon component and its scale: as its own tag or with `<svelte:component this={X}>`.
//
// Classes and icons are followed through what svelte-values.js follows within the file: names, runes, members, choices, objects, arrays and `{#each}` items of a list written in the file. Left to review: a value passed in as a prop or returned by a function, class helpers such as `clsx`, spread attributes, and class names built from pieces of a template string.

import { SvelteValues } from './svelte-values.js';

/** Where Lucide's Svelte icons come from: the package itself or one icon of it. */
const LUCIDE_SOURCE = /^@lucide\/svelte(?:\/|$)/;

/** Whether an import brings in a Lucide icon that can be rendered (not a type). */
function isLucideImport({ declaration, specifier }) {
  return (
    LUCIDE_SOURCE.test(String(declaration.source.value)) &&
    declaration.importKind !== 'type' &&
    specifier.importKind !== 'type'
  );
}

/** How each kind of value may hold an icon; `again` asks the same of a part. */
const ICON_HOLDERS = {
  ConditionalExpression: (node, again) => again(node.consequent) || again(node.alternate),
  LogicalExpression: (node, again) => again(node.left) || again(node.right),
  ObjectExpression: (node, again) =>
    node.properties.some((p) => p.type === 'Property' && again(p.value)),
  ArrayExpression: (node, again) => node.elements.some((e) => e !== null && again(e)),
};

/** Whether `expression` may evaluate to a Lucide icon. */
function holdsIcon(expression, values, seen = new Set()) {
  return values.of(expression, seen).some((value) => {
    if (value.kind === 'import') return isLucideImport(value);
    const holder = ICON_HOLDERS[value.type];
    return holder !== undefined && holder(value, (part) => holdsIcon(part, values, seen));
  });
}

function keyName(key) {
  if (key.type === 'Identifier') return [key.name];
  return key.type === 'Literal' && typeof key.value === 'string' ? [key.value] : [];
}

/** The class names each kind of class value can produce; `again` asks the same of a part. */
const CLASS_SOURCES = {
  SvelteLiteral: (node) => [node.value],
  SvelteMustacheTag: (node, again) => again(node.expression),
  Literal: (node) => (typeof node.value === 'string' ? [node.value] : []),
  TemplateLiteral: (node, again) => [
    ...node.quasis.map((q) => q.value.cooked ?? ''),
    ...node.expressions.flatMap(again),
  ],
  ConditionalExpression: (node, again) => [...again(node.consequent), ...again(node.alternate)],
  // `on && "x"` yields only its right side; `a || "x"` and `a ?? "x"` may yield either.
  LogicalExpression: (node, again) =>
    node.operator === '&&' ? again(node.right) : [...again(node.left), ...again(node.right)],
  ArrayExpression: (node, again) => node.elements.flatMap((e) => (e === null ? [] : again(e))),
  // Svelte 5 object classes: each key is a class, its value only a condition.
  ObjectExpression: (node) =>
    node.properties.flatMap((p) => (p.type === 'Property' && !p.computed ? keyName(p.key) : [])),
};

/** The class names a class value can produce: the strings in value positions only, so the `"btn"` in `kind === "btn" ? "a" : "b"` is a comparison and not a class. */
function classStrings(node, values, seen = new Set()) {
  const parts =
    node.type === 'SvelteLiteral' || node.type === 'SvelteMustacheTag'
      ? [node]
      : values.of(node, seen);
  return parts.flatMap((value) => {
    const source = CLASS_SOURCES[value.type];
    return source === undefined ? [] : source(value, (part) => classStrings(part, values, seen));
  });
}

/** The class names an element gets from its `class` attribute and `class:` directives. */
function elementClasses(element, values) {
  const names = element.startTag.attributes.flatMap((attribute) => {
    if (attribute.type === 'SvelteAttribute' && attribute.key.name === 'class') {
      return attribute.value
        .flatMap((part) => classStrings(part, values))
        .flatMap((text) => text.split(/\s+/))
        .filter((name) => name !== '');
    }
    if (attribute.type === 'SvelteDirective' && attribute.kind === 'Class')
      return [attribute.key.name.name];
    return [];
  });
  return new Set(names);
}

/** The identifier a component tag starts with: `X` in `<X>`, `L` in `<L.X>`. */
function rootIdentifier(name) {
  let node = name;
  while (node.type === 'SvelteMemberExpressionName') node = node.object;
  return node.type === 'Identifier' ? node : null;
}

/** What an element renders as a component: its tag's name, or the expression `<svelte:component this={...}>` takes; null for a plain element. */
function renderedComponent(element) {
  if (element.kind === 'component') return rootIdentifier(element.name);
  if (element.kind !== 'special' || element.name.name !== 'svelte:component') return null;
  const directive = element.startTag.attributes.find(
    (a) => a.type === 'SvelteSpecialDirective' && a.kind === 'this',
  );
  return directive?.expression ?? null;
}

/** @type {import('eslint').Rule.RuleModule} */
export const useUiPrimitives = {
  meta: {
    type: 'suggestion',
    docs: { description: 'Use the button and icon primitives instead of copying them.' },
    schema: [
      {
        type: 'object',
        properties: {
          buttonClasses: {
            type: 'object',
            description: 'Each primitive class mapped to the component that owns it.',
            additionalProperties: { type: 'string' },
          },
          icon: { type: 'string', description: 'The component that renders icons.' },
        },
        required: ['buttonClasses', 'icon'],
        additionalProperties: false,
      },
    ],
    messages: {
      primitiveClass: 'The class `{{name}}` belongs to {{component}}: use {{component}} instead.',
      directIcon:
        'Render Lucide icons through {{icon}} (`<{{icon}} icon={...} size="sm" />`), which sets their size and stroke.',
    },
  },
  create(context) {
    const [{ buttonClasses: primitives, icon }] = context.options;
    const values = new SvelteValues(context.sourceCode);
    return {
      SvelteElement(node) {
        const component = renderedComponent(node);
        if (component !== null && holdsIcon(component, values)) {
          context.report({ node: node.startTag, messageId: 'directIcon', data: { icon } });
        }
        if (node.kind === 'component') return;
        const name = [...elementClasses(node, values)].find((c) => Object.hasOwn(primitives, c));
        if (name !== undefined) {
          context.report({
            node: node.startTag,
            messageId: 'primitiveClass',
            data: { name, component: primitives[name] },
          });
        }
      },
    };
  },
};
