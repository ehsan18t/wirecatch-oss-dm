// No type assertion on a value whose type is not known.
//
// A value typed `any` or `unknown` is outside data until something checks it: a message from another script or process, what browser storage gives back, `JSON.parse`, a listener argument the browser's types leave open. `raw as Message`, `<Message>raw` and `raw!` claim a shape nothing has checked, which is the mistake a guard exists to prevent; the shape is checked with a guard instead (in the extension, the combinators in src/lib/validate.ts). Asserting a value whose type is already known (an Event to a PointerEvent) is a different decision and is left to review.
//
// Checking only that a value is an object tells nothing about its shape, so `object`, `{}` and `Record<string, unknown>` (any object type with no known members) count as unchecked too, and so does a type parameter whose constraint is one of them or that has none: after `typeof raw === 'object' && raw !== null`, `raw as Message` is still an unchecked claim. Unchecked contents of a container count the same way: `unknown[]` to `Message[]`, `Promise<unknown>` to `Promise<Message>`, `Map<string, unknown>` to `Map<string, Message>`. Asserting to a type parameter (`x as T`) claims whatever the caller picks, so it is reported too. An assertion to one of the unchecked types themselves claims nothing and stays allowed (`raw as Record<string, unknown>` before reading its fields one by one), and so do `as const` and an assertion on an object literal written in place (`{} as Record<Kind, number>`).
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

/** Whether asserting to `type` claims nothing: it is unchecked itself and names no type parameter, which a caller chooses (`x as T` claims whatever T will be). */
function claimsNothing(checker, type) {
  if ((type.flags & ts.TypeFlags.TypeParameter) !== 0) return false;
  if (type.isUnion()) return type.types.every((member) => claimsNothing(checker, member));
  return isUnchecked(checker, type);
}

/** The type arguments of a generic type such as `T[]`, `Promise<T>` or `Map<K, V>`, or none. */
function typeArguments(checker, type) {
  return (type.objectFlags ?? 0) & ts.ObjectFlags.Reference ? checker.getTypeArguments(type) : [];
}

/** Whether asserting `from` to `to` claims a shape nothing checked: at the top, or inside the type arguments of a container (`unknown[]` to `Message[]`, `Promise<unknown>` to `Promise<Message>`). */
function claimsUncheckedShape(checker, from, to) {
  if (isUnchecked(checker, from)) return !claimsNothing(checker, to);
  const fromArguments = typeArguments(checker, from);
  const toArguments = typeArguments(checker, to);
  if (fromArguments.length === 0 || fromArguments.length !== toArguments.length) return false;
  return fromArguments.some((argument, i) =>
    claimsUncheckedShape(checker, argument, toArguments[i]),
  );
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

    const report = (node, type) =>
      context.report({ node, messageId: 'assertion', data: { type: checker.typeToString(type) } });

    /** `!`: reported on any unchecked value. */
    function checkNonNull(node) {
      const type = typeOf(node.expression);
      if (isUnchecked(checker, type)) report(node, type);
    }

    /** `as` and `<T>`: reported when they claim a shape nothing checked; a literal written in place has the shape it is written with, and `as const` claims nothing. */
    function checkAssertion(node) {
      if (isConstAssertion(node) || node.expression.type === 'ObjectExpression') return;
      const type = typeOf(node.expression);
      if (claimsUncheckedShape(checker, type, typeOf(node.typeAnnotation))) report(node, type);
    }

    return {
      TSAsExpression: checkAssertion,
      TSTypeAssertion: checkAssertion,
      TSNonNullExpression: checkNonNull,
    };
  },
};
