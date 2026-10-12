// Keeps buttons and icons going through a package's primitives once it has them.
//
// A Svelte package that has a text button, an icon button and an icon component (the desktop app's
// Button, IconButton and Icon) turns this rule on everywhere except in those files. It then
// reports the two patterns the primitives replaced:
//
// - an element that carries one of the primitives' own classes (`btn`, `icon-btn`), which copies
//   the primitive by hand instead of using it. Buttons with styles of their own, such as menu
//   items and switches, stay plain buttons.
// - a Lucide icon rendered directly, with or without a size or stroke of its own, instead of
//   through the icon component and its scale.

/** Where Lucide's Svelte icons come from: the package itself or one icon of it. */
const LUCIDE_SOURCE = /^@lucide\/svelte(?:\/|$)/;

/**
 * The class names a class value can produce: the strings in value positions only, so the
 * `"btn"` in `kind === "btn" ? "a" : "b"` is a comparison and not a class.
 */
function classStrings(node) {
  switch (node?.type) {
    case 'SvelteLiteral':
      return [node.value];
    case 'SvelteMustacheTag':
      return classStrings(node.expression);
    case 'Literal':
      return typeof node.value === 'string' ? [node.value] : [];
    case 'TemplateLiteral':
      return [...node.quasis.map((q) => q.value.cooked ?? ''), ...node.expressions.flatMap(classStrings)];
    case 'ConditionalExpression':
      return [...classStrings(node.consequent), ...classStrings(node.alternate)];
    case 'LogicalExpression':
      // `on && "x"` yields only its right side; `a || "x"` and `a ?? "x"` may yield either.
      return node.operator === '&&' ? classStrings(node.right) : [...classStrings(node.left), ...classStrings(node.right)];
    case 'ArrayExpression':
      return node.elements.flatMap(classStrings);
    case 'ObjectExpression':
      // Svelte 5 object classes: each key is a class, its value only a condition.
      return node.properties.flatMap((p) => (p.type === 'Property' && !p.computed ? keyName(p.key) : []));
    default:
      return [];
  }
}

function keyName(key) {
  if (key.type === 'Identifier') return [key.name];
  return key.type === 'Literal' && typeof key.value === 'string' ? [key.value] : [];
}

/** The class names an element gets from its `class` attribute and `class:` directives. */
function elementClasses(element) {
  const names = new Set();
  for (const attribute of element.startTag.attributes) {
    if (attribute.type === 'SvelteAttribute' && attribute.key.name === 'class') {
      for (const part of attribute.value) {
        for (const text of classStrings(part)) {
          for (const name of text.split(/\s+/)) if (name !== '') names.add(name);
        }
      }
    } else if (attribute.type === 'SvelteDirective' && attribute.kind === 'Class') {
      names.add(attribute.key.name.name);
    }
  }
  return names;
}

/** The import a component tag starts with: `X` in `<X>`, `L` in `<L.X>`. */
function rootName(name) {
  let node = name;
  while (node.type === 'SvelteMemberExpressionName') node = node.object;
  return node.type === 'Identifier' ? node.name : null;
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
      directIcon: 'Render Lucide icons through {{icon}} (`<{{icon}} icon={...} size="sm" />`), which sets their size and stroke.',
    },
  },
  create(context) {
    const [{ buttonClasses: primitives, icon }] = context.options;
    const lucide = new Set();
    // The markup may come before the script that imports the icons, so components are checked
    // once the whole file has been read.
    const components = [];
    return {
      ImportDeclaration(node) {
        if (!LUCIDE_SOURCE.test(String(node.source.value)) || node.importKind === 'type') return;
        for (const specifier of node.specifiers) {
          if (specifier.importKind !== 'type') lucide.add(specifier.local.name);
        }
      },
      SvelteElement(node) {
        if (node.kind === 'component') {
          components.push(node);
          return;
        }
        const name = [...elementClasses(node)].find((c) => Object.hasOwn(primitives, c));
        if (name !== undefined) {
          context.report({ node: node.startTag, messageId: 'primitiveClass', data: { name, component: primitives[name] } });
        }
      },
      'Program:exit'() {
        for (const node of components) {
          if (lucide.has(rootName(node.name))) context.report({ node: node.startTag, messageId: 'directIcon', data: { icon } });
        }
      },
    };
  },
};
