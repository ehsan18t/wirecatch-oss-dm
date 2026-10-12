// Keeps buttons and icons going through a package's primitives once it has them.
//
// A Svelte package that has a text button, an icon button and an icon component (the desktop app's Button, IconButton and Icon) turns this rule on everywhere except in those files. It then reports the two patterns the primitives replaced:
//
// - an element that carries one of the primitives' own classes (`btn`, `icon-btn`), which copies the primitive by hand instead of using it, whether the class is written in place or comes from a name (`const cls = 'btn'; class={cls}`). Buttons with styles of their own, such as menu items and switches, stay plain buttons.
// - a Lucide icon rendered directly, with or without a size or stroke of its own, instead of through the icon component and its scale: as its own tag, through a name it was given (`const Close = X; <Close />`, `{@const}`, `$derived(...)`, an `{#each}` over icons), or with `<svelte:component this={X}>`.
//
// Names are resolved through the file's scopes, so a name that shadows another is told apart from it, and followed through other names, members, choices, objects and arrays. What the rule cannot see is a value passed into the file: an icon or a class given as a prop, or returned by a function.

/** Where Lucide's Svelte icons come from: the package itself or one icon of it. */
const LUCIDE_SOURCE = /^@lucide\/svelte(?:\/|$)/;
/** Svelte runes whose value is their first argument. */
const VALUE_RUNES = new Set(['$derived', '$state']);

/** The variable `identifier` refers to, found through the scopes the parser built, or null. */
function variableOf(sourceCode, identifier) {
  for (let scope = sourceCode.getScope(identifier); scope; scope = scope.upper) {
    const variable = scope.set.get(identifier.name);
    if (variable) return variable;
  }
  return null;
}

/** What a declaration gives its name: an import, a value, or the list an `{#each}` item comes from; null for a parameter, a prop or anything else. */
function declared(def) {
  if (def.type === 'ImportBinding')
    return { kind: 'import', key: def.node, declaration: def.parent, specifier: def.node };
  if (def.type === 'Variable' && def.node.init)
    return { kind: 'value', key: def.node, node: def.node.init };
  if (def.node.type === 'SvelteEachBlock')
    return { kind: 'value', key: def.node, node: def.node.expression };
  return null;
}

/** Follows names to their declarations; each declaration is followed once per question, so names declared from each other end. */
class Declarations {
  constructor(sourceCode) {
    this.sourceCode = sourceCode;
  }

  /** The declaration `identifier` refers to, or null when it is unknown or was already followed. */
  follow(identifier, seen) {
    const def = variableOf(this.sourceCode, identifier)?.defs[0];
    const found = def === undefined ? null : declared(def);
    if (found === null || seen.has(found.key)) return null;
    seen.add(found.key);
    return found;
  }
}

/** Whether an import brings in a Lucide icon that can be rendered (not a type). */
function isLucideImport({ declaration, specifier }) {
  return (
    LUCIDE_SOURCE.test(String(declaration.source.value)) &&
    declaration.importKind !== 'type' &&
    specifier.importKind !== 'type'
  );
}

/** Whether `node` is `$derived(value)` or `$state(value)`, whose value is its argument. */
function isRuneCall(node) {
  return node.callee.type === 'Identifier' && VALUE_RUNES.has(node.callee.name);
}

/** How each kind of expression may hold an icon; `again` asks the same of a part. */
const ICON_HOLDERS = {
  Identifier: (node, again, follow) => {
    const found = follow(node);
    if (found === null) return false;
    return found.kind === 'import' ? isLucideImport(found) : again(found.node);
  },
  MemberExpression: (node, again) => again(node.object),
  ConditionalExpression: (node, again) => again(node.consequent) || again(node.alternate),
  LogicalExpression: (node, again) => again(node.left) || again(node.right),
  ObjectExpression: (node, again) =>
    node.properties.some((p) => p.type === 'Property' && again(p.value)),
  ArrayExpression: (node, again) => node.elements.some((e) => e !== null && again(e)),
  CallExpression: (node, again) => isRuneCall(node) && again(node.arguments[0]),
};

/** Whether `expression` may evaluate to a Lucide icon. */
function holdsIcon(expression, declarations, seen = new Set()) {
  const holder = expression ? ICON_HOLDERS[expression.type] : undefined;
  if (holder === undefined) return false;
  return holder(
    expression,
    (node) => holdsIcon(node, declarations, seen),
    (identifier) => declarations.follow(identifier, seen),
  );
}

function keyName(key) {
  if (key.type === 'Identifier') return [key.name];
  return key.type === 'Literal' && typeof key.value === 'string' ? [key.value] : [];
}

/** The class names each kind of class value can produce; `again` asks the same of a part. */
const CLASS_SOURCES = {
  Identifier: (node, again, follow) => {
    const found = follow(node);
    return found?.kind === 'value' ? again(found.node) : [];
  },
  CallExpression: (node, again) => (isRuneCall(node) ? again(node.arguments[0]) : []),
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

/** The class names a class value can produce: the strings in value positions only, so the `"btn"` in `kind === "btn" ? "a" : "b"` is a comparison and not a class. Names are followed to what they were given. */
function classStrings(node, declarations, seen = new Set()) {
  const source = node ? CLASS_SOURCES[node.type] : undefined;
  if (source === undefined) return [];
  return source(
    node,
    (inner) => classStrings(inner, declarations, seen),
    (identifier) => declarations.follow(identifier, seen),
  );
}

/** The class names an element gets from its `class` attribute and `class:` directives. */
function elementClasses(element, declarations) {
  const names = element.startTag.attributes.flatMap((attribute) => {
    if (attribute.type === 'SvelteAttribute' && attribute.key.name === 'class') {
      return attribute.value
        .flatMap((part) => classStrings(part, declarations))
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
    const declarations = new Declarations(context.sourceCode);
    return {
      SvelteElement(node) {
        const component = renderedComponent(node);
        if (component !== null && holdsIcon(component, declarations)) {
          context.report({ node: node.startTag, messageId: 'directIcon', data: { icon } });
        }
        if (node.kind === 'component') return;
        const name = [...elementClasses(node, declarations)].find((c) =>
          Object.hasOwn(primitives, c),
        );
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
