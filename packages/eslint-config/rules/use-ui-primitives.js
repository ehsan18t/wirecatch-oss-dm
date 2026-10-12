// Keeps buttons and icons going through a package's primitives once it has them.
//
// A Svelte package that has a text button, an icon button and an icon component (the desktop app's Button, IconButton and Icon) turns this rule on everywhere except in those files. It then reports the two patterns the primitives replaced:
//
// - an element that carries one of the primitives' own classes (`btn`, `icon-btn`), which copies the primitive by hand instead of using it. Buttons with styles of their own, such as menu items and switches, stay plain buttons.
// - a Lucide icon rendered directly, with or without a size or stroke of its own, instead of through the icon component and its scale: as its own tag, through a name it was given (`const Close = X; <Close />`), or with `<svelte:component this={X}>`. A name is followed through members, choices, objects and arrays; an icon passed into the file as a prop is not seen.

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
      return [
        ...node.quasis.map((q) => q.value.cooked ?? ''),
        ...node.expressions.flatMap(classStrings),
      ];
    case 'ConditionalExpression':
      return [...classStrings(node.consequent), ...classStrings(node.alternate)];
    case 'LogicalExpression':
      // `on && "x"` yields only its right side; `a || "x"` and `a ?? "x"` may yield either.
      return node.operator === '&&'
        ? classStrings(node.right)
        : [...classStrings(node.left), ...classStrings(node.right)];
    case 'ArrayExpression':
      return node.elements.flatMap(classStrings);
    case 'ObjectExpression':
      // Svelte 5 object classes: each key is a class, its value only a condition.
      return node.properties.flatMap((p) =>
        p.type === 'Property' && !p.computed ? keyName(p.key) : [],
      );
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
  return new Set(element.startTag.attributes.flatMap(attributeClasses));
}

/** The class names one attribute gives an element. */
function attributeClasses(attribute) {
  if (attribute.type === 'SvelteAttribute' && attribute.key.name === 'class') {
    return attribute.value
      .flatMap(classStrings)
      .flatMap((text) => text.split(/\s+/))
      .filter((name) => name !== '');
  }
  if (attribute.type === 'SvelteDirective' && attribute.kind === 'Class')
    return [attribute.key.name.name];
  return [];
}

/** The name a component tag starts with: `X` in `<X>`, `L` in `<L.X>`. */
function rootName(name) {
  let node = name;
  while (node.type === 'SvelteMemberExpressionName') node = node.object;
  return node.type === 'Identifier' ? node.name : null;
}

/** The expression `<svelte:component this={...}>` renders, or null for any other element. */
function dynamicComponent(element) {
  if (element.kind !== 'special' || element.name.name !== 'svelte:component') return null;
  const directive = element.startTag.attributes.find(
    (a) => a.type === 'SvelteSpecialDirective' && a.kind === 'this',
  );
  return directive?.expression ?? null;
}

/** Which names hold a Lucide icon: the icons a file imports, and every name given one of them, directly or through a member, a choice, an object or an array (`const Close = X`, `{@const I = on ? A : B}`, `const icons = { close: X }`). */
class IconNames {
  imported = new Set();
  /** Each `const name = init` in the file, script and markup alike. */
  declared = new Map();

  /** Whether `expression` may evaluate to a Lucide icon. */
  holdsIcon(expression, seen = new Set()) {
    switch (expression?.type) {
      case 'Identifier':
        return this.nameHoldsIcon(expression.name, seen);
      case 'MemberExpression':
        return this.holdsIcon(expression.object, seen);
      case 'ConditionalExpression':
        return (
          this.holdsIcon(expression.consequent, seen) || this.holdsIcon(expression.alternate, seen)
        );
      case 'LogicalExpression':
        return this.holdsIcon(expression.left, seen) || this.holdsIcon(expression.right, seen);
      case 'ObjectExpression':
        return expression.properties.some(
          (p) => p.type === 'Property' && this.holdsIcon(p.value, seen),
        );
      case 'ArrayExpression':
        return expression.elements.some((e) => this.holdsIcon(e, seen));
      default:
        return false;
    }
  }

  nameHoldsIcon(name, seen) {
    if (this.imported.has(name)) return true;
    if (seen.has(name) || !this.declared.has(name)) return false;
    seen.add(name);
    return this.holdsIcon(this.declared.get(name), seen);
  }
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
    const icons = new IconNames();
    // The markup may come before the script that imports or names the icons, so what is rendered is checked once the whole file has been read.
    const rendered = [];
    return {
      ImportDeclaration(node) {
        if (!LUCIDE_SOURCE.test(String(node.source.value)) || node.importKind === 'type') return;
        for (const specifier of node.specifiers) {
          if (specifier.importKind !== 'type') icons.imported.add(specifier.local.name);
        }
      },
      VariableDeclarator(node) {
        if (node.id.type === 'Identifier' && node.init) icons.declared.set(node.id.name, node.init);
      },
      SvelteElement(node) {
        if (node.kind === 'component') {
          const name = rootName(node.name);
          if (name !== null) rendered.push({ node, expression: { type: 'Identifier', name } });
          return;
        }
        const expression = dynamicComponent(node);
        if (expression !== null) rendered.push({ node, expression });
        const name = [...elementClasses(node)].find((c) => Object.hasOwn(primitives, c));
        if (name !== undefined) {
          context.report({
            node: node.startTag,
            messageId: 'primitiveClass',
            data: { name, component: primitives[name] },
          });
        }
      },
      'Program:exit'() {
        for (const { node, expression } of rendered) {
          if (icons.holdsIcon(expression)) {
            context.report({ node: node.startTag, messageId: 'directIcon', data: { icon } });
          }
        }
      },
    };
  },
};
