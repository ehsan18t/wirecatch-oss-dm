// No type assertion on a value whose type is not known.
//
// A value typed `any` or `unknown` is outside data until something checks it: a message from
// another script or process, what browser storage gives back, `JSON.parse`, a listener argument
// the browser's types leave open. `raw as Message`, `<Message>raw` and `raw!` claim a shape nothing
// has checked, which is the mistake a guard exists to prevent; the shape is checked with a guard
// instead (in the extension, the combinators in src/lib/validate.ts). Asserting a value whose type
// is already known (an Event to a PointerEvent) is a different decision and is left to review.
// `as const` claims nothing about the value and stays allowed.
import ts from 'typescript';

/** Type flags of a value nothing has checked. */
const UNCHECKED = ts.TypeFlags.Any | ts.TypeFlags.Unknown;

/** Whether an assertion is `as const` (or `<const>`), which only fixes a literal's type. */
function isConstAssertion(node) {
  const type = node.typeAnnotation;
  return type.type === 'TSTypeReference' && type.typeName.type === 'Identifier' && type.typeName.name === 'const';
}

/** @type {import('eslint').Rule.RuleModule} */
export const noAssertionOnInput = {
  meta: {
    type: 'problem',
    docs: { description: 'Disallow type and non-null assertions on values typed `any` or `unknown`.' },
    schema: [],
    messages: {
      assertion: 'This value is typed `{{type}}`: check its shape with a guard instead of asserting it.',
    },
  },
  create(context) {
    const services = context.sourceCode.parserServices;
    if (!services?.program) {
      throw new Error('wirecatch/no-assertion-on-input needs type information (parserOptions.projectService).');
    }
    const checker = services.program.getTypeChecker();

    function check(node) {
      const type = checker.getTypeAtLocation(services.esTreeNodeToTSNodeMap.get(node.expression));
      if ((type.flags & UNCHECKED) === 0) return;
      context.report({ node, messageId: 'assertion', data: { type: checker.typeToString(type) } });
    }

    return {
      TSAsExpression(node) {
        if (!isConstAssertion(node)) check(node);
      },
      TSTypeAssertion(node) {
        if (!isConstAssertion(node)) check(node);
      },
      TSNonNullExpression: check,
    };
  },
};
