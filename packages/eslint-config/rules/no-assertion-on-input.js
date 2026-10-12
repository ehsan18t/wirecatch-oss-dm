// No type assertion where outside data is checked.
//
// A function that takes a parameter typed `unknown`, or that is a type guard (`value is T`), is
// where a message, a stored value or an answer from another process becomes a type. An `as` there
// claims a shape nothing has checked, which is exactly the mistake such a function exists to
// prevent; the shape is checked with a guard instead (in the extension, the combinators in
// src/lib/validate.ts). `as const` claims nothing about outside data and stays allowed.

/** Function nodes whose parameters and return type say what they accept. */
const FUNCTIONS = new Set(['FunctionDeclaration', 'FunctionExpression', 'ArrowFunctionExpression']);

/** The type annotation of a parameter, looking through defaults and `...rest`. */
function annotationOf(param) {
  if (param.type === 'AssignmentPattern') return annotationOf(param.left);
  if (param.type === 'TSParameterProperty') return annotationOf(param.parameter);
  return param.typeAnnotation?.typeAnnotation;
}

/** Whether `fn` reads outside data: an `unknown` parameter, or a type guard's return type. */
function readsInput(fn) {
  if (fn.returnType?.typeAnnotation.type === 'TSTypePredicate') return true;
  return fn.params.some((param) => annotationOf(param)?.type === 'TSUnknownKeyword');
}

/** Whether an assertion is `as const` (or `<const>`), which only fixes a literal's type. */
function isConstAssertion(node) {
  const type = node.typeAnnotation;
  return type.type === 'TSTypeReference' && type.typeName.type === 'Identifier' && type.typeName.name === 'const';
}

/** @type {import('eslint').Rule.RuleModule} */
export const noAssertionOnInput = {
  meta: {
    type: 'problem',
    docs: { description: 'Disallow type assertions in functions that check outside data.' },
    schema: [],
    messages: {
      assertion:
        'This function checks outside data: check the shape with a guard instead of asserting it with `as`.',
    },
  },
  create(context) {
    function check(node) {
      if (isConstAssertion(node)) return;
      const inInputFunction = context.sourceCode
        .getAncestors(node)
        .some((ancestor) => FUNCTIONS.has(ancestor.type) && readsInput(ancestor));
      if (inInputFunction) context.report({ node, messageId: 'assertion' });
    }
    return { TSAsExpression: check, TSTypeAssertion: check };
  },
};
