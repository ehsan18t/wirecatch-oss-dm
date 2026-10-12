// No type assertion on a value whose type is not known.
//
// A value typed `any` or `unknown` is outside data until something checks it: a message from another script or process, what browser storage gives back, `JSON.parse`, a listener argument the browser's types leave open. `raw as Message`, `<Message>raw` and `raw!` claim a shape nothing has checked, which is the mistake a guard exists to prevent; the shape is checked with a guard instead (in the extension, the combinators in src/lib/validate.ts). Asserting a value whose type is already known (an Event to a PointerEvent) is a different decision and is left to review.
//
// Checking only that a value is an object tells nothing about its shape, so `object`, `{}` and `Record<string, unknown>` (any object type with no known members) count as unchecked too, and so does a type parameter whose constraint is one of them or that has none: after `typeof raw === 'object' && raw !== null`, `raw as Message` is still an unchecked claim. An assertion to one of those types claims nothing and stays allowed (`raw as Record<string, unknown>` before reading its fields one by one), and so do `as const` and an assertion on an object literal written in place (`{} as Record<Kind, number>`).
import ts from 'typescript';

/** Type flags of a value nothing has checked. */
const UNCHECKED = ts.TypeFlags.Any | ts.TypeFlags.Unknown | ts.TypeFlags.NonPrimitive;

/** Whether an assertion is `as const` (or `<const>`), which only fixes a literal's type. */
function isConstAssertion(node) {
  const type = node.typeAnnotation;
  return (
    type.type === 'TSTypeReference' &&
    type.typeName.type === 'Identifier' &&
    type.typeName.name === 'const'
  );
}

/** Whether `type` is an object type with no known members: `{}`, or only index signatures of unchecked values. */
function isShapelessObject(checker, type) {
  if ((type.flags & ts.TypeFlags.Object) === 0) return false;
  return (
    checker.getPropertiesOfType(type).length === 0 &&
    checker.getSignaturesOfType(type, ts.SignatureKind.Call).length === 0 &&
    checker.getSignaturesOfType(type, ts.SignatureKind.Construct).length === 0 &&
    checker.getIndexInfosOfType(type).every((info) => isUnchecked(checker, info.type))
  );
}

/** Whether a value of `type` may be anything as far as its shape goes. */
function isUnchecked(checker, type) {
  if ((type.flags & UNCHECKED) !== 0) return true;
  if ((type.flags & ts.TypeFlags.TypeParameter) !== 0) {
    const constraint = checker.getBaseConstraintOfType(type);
    return constraint === undefined || constraint === type || isUnchecked(checker, constraint);
  }
  if (type.isUnion()) return type.types.some((member) => isUnchecked(checker, member));
  return isShapelessObject(checker, type);
}

/** @type {import('eslint').Rule.RuleModule} */
export const noAssertionOnInput = {
  meta: {
    type: 'problem',
    docs: {
      description:
        'Disallow type and non-null assertions on values whose shape is unchecked (`any`, `unknown`, `object`, `{}`, `Record<string, unknown>`).',
    },
    schema: [],
    messages: {
      assertion:
        'This value is typed `{{type}}`, which says nothing about its shape: check it with a guard instead of asserting it.',
    },
  },
  create(context) {
    const services = context.sourceCode.parserServices;
    if (!services?.program) {
      throw new Error(
        'wirecatch/no-assertion-on-input needs type information (parserOptions.projectService).',
      );
    }
    const checker = services.program.getTypeChecker();
    const typeOf = (node) => checker.getTypeAtLocation(services.esTreeNodeToTSNodeMap.get(node));

    function check(node) {
      // A literal written right here has the shape it is written with, whatever its type says.
      if (node.expression.type === 'ObjectExpression') return;
      const type = typeOf(node.expression);
      if (!isUnchecked(checker, type)) return;
      context.report({ node, messageId: 'assertion', data: { type: checker.typeToString(type) } });
    }

    /** `as` and `<T>`: allowed when they fix a literal or assert to a type that is itself unchecked. */
    function checkAssertion(node) {
      if (isConstAssertion(node) || isUnchecked(checker, typeOf(node.typeAnnotation))) return;
      check(node);
    }

    return {
      TSAsExpression: checkAssertion,
      TSTypeAssertion: checkAssertion,
      TSNonNullExpression: check,
    };
  },
};
