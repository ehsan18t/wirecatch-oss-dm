// A caught error is handled, never dropped without a word (docs/ENGINEERING.md, section 6).
//
// Handled means one of three things, decided by what the handler does:
// - the error goes somewhere: it is passed to a call (the logger, a wrapper, `reject`), thrown,
//   returned, or assigned to something outside the handler, itself or a value made from it (a
//   field, `String(e)`, a template, an object or a list holding it). A value made from it and then
//   dropped (`String(e);`, `[e];`, `e.message;`, a `const` never read), `void e` and a test
//   (`if (e)`) do not count: the error itself still goes nowhere.
// - the handler is a function of this file that handles its error by these same rules, or it is
//   made by a factory the configuration names and the file imports from the module named with it
//   (the extension's `fallback` from `diag/log`, which logs). Any other call is not trusted.
// - a comment directly inside the handler, not in a function nested in it, says in words why the
//   error is expected. An empty comment, a `TODO` or a lint directive is not a reason.
//
// Checked: `catch` clauses, and the rejection handlers of `promise.catch(handler)` and
// `promise.then(onFulfilled, handler)`, also written `promise['catch'](handler)`. Any other
// handler (an imported name, a parameter, `Boolean`, a method such as `console.error`, a call
// that is not a named factory) cannot be checked here and is reported.

const FUNCTION_TYPES = new Set([
  'ArrowFunctionExpression',
  'FunctionExpression',
  'FunctionDeclaration',
]);
/** Nodes that pass a value on unchanged, so what counts is what their parent does with it. */
const TRANSPARENT = new Set([
  'TSAsExpression',
  'TSNonNullExpression',
  'TSSatisfiesExpression',
  'ChainExpression',
]);
/** Calls that only convert a value, so their result is the error in another form. */
const CONVERSIONS = new Set(['String', 'Boolean', 'Number']);
/** How many `const` copies of the error are followed before giving up and counting it used. */
const MAX_COPIES = 4;
/** How many words a comment needs to give a reason. */
const REASON_WORDS = 3;
/** Comments that are not reasons: lint and type directives, and notes of work still to do. */
const NOT_A_REASON = /^(eslint|global|exported|jshint|@ts-)|\b(todo|fixme|xxx|hack)\b/i;
/** Literals made from the error whose member is something else: `[e][1]`, `{ e }.x`. */
const LITERALS = new Set(['ArrayExpression', 'ObjectExpression']);

/** Whether a call only converts its argument (`String(e)`). */
const isConversion = (call) =>
  call.callee.type === 'Identifier' && CONVERSIONS.has(call.callee.name);

/**
 * What a parent does with the value of its child `node`: `true` hands it on, `'derived'` makes a
 * value from it whose fate its own parent decides, `'returned'` makes it a function's result,
 * `'copied'` keeps it in a variable, and anything else drops or only tests it.
 */
const FATE = {
  CallExpression: (parent, node) => {
    if (parent.callee === node) return 'derived';
    if (!parent.arguments.includes(node)) return false;
    return isConversion(parent) ? 'derived' : true;
  },
  // What is built from the error goes where the built value goes: `throw new Wrapped(e)` hands
  // it on, `new Set([e]);` drops it.
  NewExpression: (parent, node) => (parent.arguments.includes(node) ? 'derived' : false),
  ArrowFunctionExpression: (parent, node) => (parent.body === node ? 'returned' : false),
  AssignmentExpression: (parent, node) => parent.right === node,
  VariableDeclarator: (parent, node) => (parent.init === node ? 'copied' : false),
  MemberExpression: (parent, node) =>
    parent.object === node && !LITERALS.has(node.type) ? 'derived' : false,
  Property: (parent, node) => (parent.value === node ? 'derived' : false),
  ConditionalExpression: (parent, node) => (parent.test === node ? false : 'derived'),
  // `e && x` is never the error; `e || x`, `e ?? x` and `x && e` can be.
  LogicalExpression: (parent, node) =>
    parent.operator === '&&' && parent.left === node ? false : 'derived',
  ThrowStatement: () => true,
  ReturnStatement: () => 'returned',
  YieldExpression: () => true,
  ArrayExpression: () => 'derived',
  ObjectExpression: () => 'derived',
  SpreadElement: () => 'derived',
  TemplateLiteral: () => 'derived',
  BinaryExpression: () => 'derived',
  AwaitExpression: () => 'derived',
};

/** The function a node sits in, or null at the top of the file. */
function enclosingFunction(node) {
  let current = node.parent;
  while (current !== null && current !== undefined && !FUNCTION_TYPES.has(current.type)) {
    current = current.parent;
  }
  return current ?? null;
}

/** Whether `inner` lies within `outer`. */
const within = (inner, outer) =>
  inner.range[0] >= outer.range[0] && inner.range[1] <= outer.range[1];

/**
 * What becomes of a value `fn` returns. Returned by the rejection handler itself, it becomes the
 * promise's value, which nobody need read: dropped. Returned by a function written inside the
 * handler, it is that function's fate: the function is followed instead, or, for a declaration,
 * the name it is declared by. Returned by the function a `catch` clause sits in, it reaches the
 * caller: handed on.
 */
function returnedBy(fn, walk) {
  if (fn === null || fn === walk.handler) return { done: true, handed: fn === null };
  if (!within(fn, walk.handler)) return { done: true, handed: true };
  if (fn.type === 'FunctionDeclaration') return { done: true, handed: declarationIsUsed(walk, fn) };
  return { done: false, node: fn };
}

/** Whether a function declared inside the handler is handed on, or called for a result that is. */
function declarationIsUsed(walk, fn) {
  if (walk.copies >= MAX_COPIES || fn.id === null) return true;
  return anyHandedOn({ ...walk, copies: walk.copies + 1 }, fn, fn.id);
}

/** Whether the value of `start` (the error, or a value made from it) goes somewhere. */
function handsOn(walk, start) {
  let node = start;
  for (;;) {
    while (TRANSPARENT.has(node.parent.type)) node = node.parent;
    const parent = node.parent;
    const fate = Object.hasOwn(FATE, parent.type) ? FATE[parent.type](parent, node) : false;
    if (fate === 'derived') node = parent;
    else if (fate === 'copied') return copyIsUsed(walk, parent);
    else if (fate === 'returned') {
      const fn = parent.type === 'ReturnStatement' ? enclosingFunction(parent) : parent;
      const next = returnedBy(fn, walk);
      if (next.done) return next.handed;
      node = next.node;
    } else return fate === true;
  }
}

/** The names a parameter or a declaration binds: `e`, `e = null`, `...e`, `{ message }`, `[first]`. */
function boundNames(pattern) {
  switch (pattern?.type) {
    case 'Identifier':
      return [pattern];
    case 'AssignmentPattern':
      return boundNames(pattern.left);
    case 'RestElement':
      return boundNames(pattern.argument);
    case 'ArrayPattern':
      return pattern.elements.flatMap(boundNames);
    case 'ObjectPattern':
      return pattern.properties.flatMap((p) => boundNames(p.type === 'RestElement' ? p : p.value));
    default:
      return [];
  }
}

/** Whether any name `pattern` binds, as declared by `node`, is handed on. */
function anyHandedOn(walk, node, pattern) {
  const names = boundNames(pattern);
  return walk.context.sourceCode
    .getDeclaredVariables(node)
    .filter((variable) => variable.identifiers.some((id) => names.includes(id)))
    .some((variable) =>
      variable.references.some(
        (reference) => reference.isRead() && handsOn(walk, reference.identifier),
      ),
    );
}

/** Whether a variable holding the error, or a value made from it, is itself handed on. */
function copyIsUsed(walk, declarator) {
  if (walk.copies >= MAX_COPIES) return true;
  return anyHandedOn({ ...walk, copies: walk.copies + 1 }, declarator, declarator.id);
}

/** Whether the handler hands its error on, through whatever names its parameter binds. */
function usesError(context, handler) {
  const parameter = handler.type === 'CatchClause' ? handler.param : handler.params[0];
  if (parameter === null || parameter === undefined) return false;
  return anyHandedOn({ context, handler, copies: 0 }, handler, parameter);
}

/** Whether `comment` sits directly in `handler`, not inside a function nested in it. */
function directlyIn(context, comment, handler) {
  let node = context.sourceCode.getNodeByRangeIndex(comment.range[0]);
  while (node !== null && node !== handler) {
    if (FUNCTION_TYPES.has(node.type)) return false;
    node = node.parent;
  }
  return node === handler;
}

/** Whether a comment written in the handler says why its error is expected. */
function explained(context, handler) {
  const body = handler.type === 'CatchClause' ? handler.body : handler;
  return context.sourceCode.getCommentsInside(body).some((comment) => {
    const text = comment.value.replace(/^[\s*]+/, '').trim();
    const words = text.match(/\p{L}+/gu) ?? [];
    return (
      !NOT_A_REASON.test(text) && words.length >= REASON_WORDS && directlyIn(context, comment, body)
    );
  });
}

/** Whether a handler function handles its error. */
function handles(context, handler) {
  return usesError(context, handler) || explained(context, handler);
}

/** The variable `identifier` names, from the innermost scope that declares it; null when none does. */
function variableOf(context, identifier) {
  for (let scope = context.sourceCode.getScope(identifier); scope !== null; scope = scope.upper) {
    const variable = scope.set.get(identifier.name);
    if (variable !== undefined) return variable;
  }
  return null;
}

/**
 * What a name stands for as a handler: the function this file declares under it, or the call a
 * `const` of it is made by. Null for anything else (a parameter, an import, a `let`), whose value
 * this file does not decide.
 */
function localHandler(context, identifier) {
  const definition = variableOf(context, identifier)?.defs[0];
  if (definition?.type === 'FunctionName') return definition.node;
  if (definition?.type !== 'Variable' || definition.parent.kind !== 'const') return null;
  const init = definition.node.init;
  return FUNCTION_TYPES.has(init?.type) || init?.type === 'CallExpression' ? init : null;
}

/** The name an import specifier imports, `fallback` in `import { fallback as f }`. */
const importedName = (specifier) =>
  specifier.type === 'ImportSpecifier'
    ? (specifier.imported.name ?? specifier.imported.value)
    : null;

/** Whether an import source is `module` or a path ending in it, with or without an extension. */
const isFrom = (source, module) => {
  const bare = source.replace(/\.[cm]?[jt]s$/, '');
  return bare === module || bare.endsWith(`/${module}`);
};

/** Whether `call` calls a factory the configuration names, imported from its module. */
function isFactoryCall(context, call, factories) {
  if (call.callee.type !== 'Identifier') return false;
  const definition = variableOf(context, call.callee)?.defs[0];
  if (definition?.type !== 'ImportBinding') return false;
  const name = importedName(definition.node);
  const source = definition.parent.source.value;
  return factories.some((factory) => factory.name === name && isFrom(source, factory.module));
}

/** `undefined` or `null` given as the handler: there is none, and the rejection goes on. */
const isNoHandler = (handler) =>
  (handler.type === 'Identifier' && handler.name === 'undefined') ||
  (handler.type === 'Literal' && handler.value === null);

/** The name of a member, `p.catch` or `p['catch']`; null when it is computed from something else. */
function memberName(callee) {
  if (!callee.computed) return callee.property.name;
  const key = callee.property;
  if (key.type === 'Literal' && typeof key.value === 'string') return key.value;
  if (key.type === 'TemplateLiteral' && key.expressions.length === 0)
    return key.quasis[0].value.cooked;
  return null;
}

/** The rejection handler of `promise.catch(handler)` or `promise.then(onFulfilled, handler)`. */
function rejectionHandler(call) {
  if (call.callee.type !== 'MemberExpression') return null;
  const index = { catch: 0, then: 1 }[memberName(call.callee)];
  return index === undefined ? null : (call.arguments[index] ?? null);
}

/** @type {import('eslint').Rule.RuleModule} */
export const noSilentCatch = {
  meta: {
    type: 'problem',
    docs: {
      description:
        'Disallow catching an error without using it or saying why it is expected (docs/ENGINEERING.md, section 6)',
    },
    schema: [
      {
        type: 'object',
        properties: {
          factories: {
            description:
              'Functions that make a rejection handler which logs: `name` as exported by `module`, a path matched at its end.',
            type: 'array',
            items: {
              type: 'object',
              properties: { name: { type: 'string' }, module: { type: 'string' } },
              required: ['name', 'module'],
              additionalProperties: false,
            },
          },
        },
        additionalProperties: false,
      },
    ],
    messages: {
      silent:
        'This error is dropped silently. Log it (`log` or `fallback` from the logger) or use it; only when it is expected, say in words why in a comment directly inside the handler.',
      unchecked:
        'This rejection handler cannot be checked here. Write the handler in place, use a function declared in this file, or `fallback(...)` from the logger.',
    },
  },
  create(context) {
    const factories = context.options[0]?.factories ?? [];
    const trusted = (call) => isFactoryCall(context, call, factories);
    const checkHandler = (handler) => {
      if (isNoHandler(handler)) return;
      const local = handler.type === 'Identifier' ? localHandler(context, handler) : handler;
      if (local !== null && FUNCTION_TYPES.has(local.type)) {
        if (!handles(context, local)) context.report({ node: handler, messageId: 'silent' });
      } else if (local?.type !== 'CallExpression' || !trusted(local)) {
        context.report({ node: handler, messageId: 'unchecked' });
      }
    };
    return {
      CatchClause(node) {
        if (!handles(context, node)) context.report({ node, messageId: 'silent' });
      },
      CallExpression(node) {
        const handler = rejectionHandler(node);
        if (handler !== null) checkHandler(handler);
      },
    };
  },
};
