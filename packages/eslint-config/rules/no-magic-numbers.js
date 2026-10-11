// `@typescript-eslint/no-magic-numbers`, except for numbers that build a named module constant.
//
// The base rule counts a number as named only when it is the whole initializer of a declaration,
// so it reports the parts of `const MAX_URL_LENGTH = 8 * 1024` and `const BACKOFF = [1_000, 2_000]`.
// docs/ENGINEERING.md, section 2, asks for exactly those: a value one module uses is a `const` at
// the top of that module, written the way it reads. This rule lets a number through when nothing
// but arithmetic, a sign, an array, an object or a type assertion stands between it and a
// module-level `const`. Numbers inside functions, calls and conditions are still reported.
import ts from 'typescript-eslint';

const base = ts.plugin.rules['no-magic-numbers'];

/** Nodes that only build a value out of its parts, and so keep the name of the constant they sit in. */
const BUILDING_NODES = new Set([
  'UnaryExpression',
  'BinaryExpression',
  'ArrayExpression',
  'ObjectExpression',
  'Property',
  'TSAsExpression',
  'TSSatisfiesExpression',
]);

/** Where a top-level declaration sits: the module itself, an `export` around it, or a Svelte component's script. */
const MODULE_LEVEL = new Set(['Program', 'ExportNamedDeclaration', 'SvelteScriptElement']);

/** Whether `literal` is part of the value of a module-level `const`. */
function inNamedConstant(literal) {
  let child = literal;
  let node = literal.parent;
  while (node !== undefined && node !== null && BUILDING_NODES.has(node.type)) {
    // In an object, only the value counts: a computed key is not part of what the constant holds.
    if (node.type === 'Property' && node.value !== child) return false;
    child = node;
    node = node.parent;
  }
  if (node?.type !== 'VariableDeclarator' || node.init !== child) return false;
  const declaration = node.parent;
  return declaration.kind === 'const' && MODULE_LEVEL.has(declaration.parent?.type);
}

/** @type {import('eslint').Rule.RuleModule} */
export const noMagicNumbers = {
  meta: {
    ...base.meta,
    docs: {
      description:
        'Disallow magic numbers, except in the value of a named module constant (docs/ENGINEERING.md, section 2)',
    },
  },
  defaultOptions: base.defaultOptions,
  create(context) {
    const filtered = Object.create(context, {
      report: {
        value(descriptor) {
          if (descriptor.node !== undefined && inNamedConstant(descriptor.node)) return;
          context.report(descriptor);
        },
      },
    });
    return base.create(filtered);
  },
};
