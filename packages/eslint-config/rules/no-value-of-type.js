// No value of the named types in the files a package layer gives this rule.
//
// A layer rule written as a ban on imports or on one property name is easy to step around: `const s = app; s.backend`, `getApp().backend`, `app[key]`, `const { backend } = app`, `Reflect.get(app, 'backend')`. This rule asks the type checker instead. The desktop app uses it to keep its components from the backend: they show state and report what the person did, and the state classes call the backend.
//
// Reported, when its type is one of the named types, a class or interface extending one, or holds one (a union, or the type arguments of a generic such as `Promise<T>`, a Svelte store `Readable<T>`, `Pick<T, K>` or `Partial<T>`): a member access, a call, an `await`, an `as` expression, a reference to a name, a destructured name (including from Svelte's `$props()`) and a parameter; and `Reflect.get` on an object that has a property of such a type. A value is reported where it first appears, not again at each use.
//
// Left to review: names bound by an `{#each}` or a snippet's parameters, a function type returning the value, and a look-alike type with the same members under another name (the checker compares names, not shapes).
import ts from 'typescript';

/** The types `type` is made from: union and intersection members, type arguments (of `Promise<T>`, `Readable<T>`, an alias such as `Pick<T, K>`), and the classes or interfaces it extends. */
function partsOf(checker, type) {
  if (type.isUnion() || type.isIntersection()) return type.types;
  const parts = [...(type.aliasTypeArguments ?? [])];
  if ((type.flags & ts.TypeFlags.Object) === 0) return parts;
  if (type.objectFlags & ts.ObjectFlags.Reference) parts.push(...checker.getTypeArguments(type));
  if (type.objectFlags & ts.ObjectFlags.ClassOrInterface)
    parts.push(...(checker.getBaseTypes(type) ?? []));
  return parts;
}

/** Whether `type` is a named type or is made from one. `seen` stops recursive types. */
function hasBannedType(checker, type, names, seen = new Set()) {
  if (seen.has(type)) return false;
  seen.add(type);
  const symbol = type.getSymbol() ?? type.aliasSymbol;
  if (symbol && names.has(symbol.getName())) return true;
  return partsOf(checker, type).some((part) => hasBannedType(checker, part, names, seen));
}

/** The names a pattern binds: `a` in `a`, `{ a, b: c }`, `[a, ...d]`, `{ a = 1 }`. */
function boundNames(pattern) {
  switch (pattern?.type) {
    case 'Identifier':
      return [pattern];
    case 'ObjectPattern':
      return pattern.properties.flatMap((p) =>
        boundNames(p.type === 'RestElement' ? p.argument : p.value),
      );
    case 'ArrayPattern':
      return pattern.elements.flatMap(boundNames);
    case 'AssignmentPattern':
      return boundNames(pattern.left);
    case 'RestElement':
      return boundNames(pattern.argument);
    default:
      return [];
  }
}

/** Whether `node` is `Reflect.get(...)`. */
function isReflectGet(node) {
  const callee = node.callee;
  return (
    callee.type === 'MemberExpression' &&
    callee.object.type === 'Identifier' &&
    callee.object.name === 'Reflect' &&
    callee.property.type === 'Identifier' &&
    callee.property.name === 'get'
  );
}

/** @type {import('eslint').Rule.RuleModule} */
export const noValueOfType = {
  meta: {
    type: 'problem',
    docs: { description: 'Disallow values of the named types in the files the rule is given.' },
    schema: [
      {
        type: 'object',
        properties: {
          types: { type: 'array', items: { type: 'string' }, minItems: 1 },
          message: { type: 'string', description: 'What to do instead.' },
        },
        required: ['types', 'message'],
        additionalProperties: false,
      },
    ],
    messages: { banned: 'This is a `{{type}}`. {{message}}' },
  },
  create(context) {
    const { sourceCode } = context;
    const services = sourceCode.parserServices;
    if (!services?.program) {
      throw new Error(
        'wirecatch/no-value-of-type needs type information (parserOptions.projectService).',
      );
    }
    const [{ types, message }] = context.options;
    const names = new Set(types);
    const checker = services.program.getTypeChecker();
    const typeOf = (node) => checker.getTypeAtLocation(services.esTreeNodeToTSNodeMap.get(node));
    /** A value is reported where it first appears, not again at the expression around it or at each use of the name that holds it. */
    const reported = new WeakSet();

    function check(node) {
      const type = typeOf(node);
      if (!hasBannedType(checker, type, names)) return;
      reported.add(node);
      context.report({
        node,
        messageId: 'banned',
        data: { type: checker.typeToString(type), message },
      });
    }

    const checkAround = (inner) => (node) => {
      if (!reported.has(node[inner])) check(node);
    };

    function checkReflect(node) {
      const target = node.arguments[0];
      if (target === undefined || target.type === 'SpreadElement') return;
      const location = services.esTreeNodeToTSNodeMap.get(target);
      const holds = checker
        .getPropertiesOfType(typeOf(target))
        .some((property) =>
          hasBannedType(checker, checker.getTypeOfSymbolAtLocation(property, location), names),
        );
      if (holds)
        context.report({
          node,
          messageId: 'banned',
          data: { type: 'value reached with Reflect.get', message },
        });
    }

    function checkReference(node) {
      const reference = sourceCode.getScope(node).references.find((r) => r.identifier === node);
      if (!reference?.isValueReference || !reference.isRead()) return;
      if (reference.resolved?.defs.some((def) => reported.has(def.name))) return;
      check(node);
    }

    const checkBindings = (pattern) => {
      for (const name of boundNames(pattern)) check(name);
    };

    return {
      MemberExpression: check,
      CallExpression(node) {
        if (isReflectGet(node)) checkReflect(node);
        else check(node);
      },
      'AwaitExpression:exit': checkAround('argument'),
      'TSAsExpression:exit': checkAround('expression'),
      Identifier: checkReference,
      VariableDeclarator(node) {
        // `const b = app.backend` is reported at its value; a destructured name has no value of its own.
        if (node.id.type !== 'Identifier') checkBindings(node.id);
      },
      'VariableDeclarator:exit'(node) {
        if (node.id.type === 'Identifier' && node.init && reported.has(node.init))
          reported.add(node.id);
      },
      'FunctionDeclaration, FunctionExpression, ArrowFunctionExpression'(node) {
        for (const param of node.params) checkBindings(param);
      },
    };
  },
};
