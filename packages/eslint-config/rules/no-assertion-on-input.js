// No type assertion on a value whose type is not known.
//
// A value typed `any` or `unknown` is outside data until something checks it: a message from another script or process, what browser storage gives back, `JSON.parse`, a listener argument the browser's types leave open. `raw as Message`, `<Message>raw` and `raw!` claim a shape nothing has checked, which is the mistake a guard exists to prevent; the shape is checked with a guard instead (in the extension, the combinators in src/lib/validate.ts). Asserting a value whose type is already known (an Event to a PointerEvent) is a different decision and is left to review.
//
// Checking only that a value is an object tells nothing about its shape, so `object`, `{}` and `Record<string, unknown>` count as unchecked too, and so does a type parameter whose constraint is one of them or that has none. The claim is also found inside the assertion: member by member of a union (`unknown[] | undefined` to `Message[] | undefined`), in the type arguments of a container (`Promise<unknown>` to `Promise<Message>`), from an array into a tuple, and in a property of the same name (`{ data: unknown }` to `{ data: Message }`). Asserting to a type parameter (`x as T`) claims whatever the caller picks.
//
// Allowed: an assertion to an unchecked type itself (`raw as Record<string, unknown>` before reading its fields), `as const`, and an assertion on an object literal, an empty array literal or a constructor called with no arguments (`{}`, `[]`, `new Map()`), whose contents are written right there, though their type arguments say `any`. An array with elements or a constructor with arguments can carry outside input (`[...raw]`, `new Set(raw)`) and is checked like any other value.
//
// Left to review: a generic type alias whose arguments the checker does not keep, and a cast of what a function returns to another function type.
import ts from 'typescript';

/** Type flags of a value nothing has checked. */
const UNCHECKED = ts.TypeFlags.Any | ts.TypeFlags.Unknown | ts.TypeFlags.NonPrimitive;

/** Whether `node` is written in the assertion with no outside contents: an object literal, `[]`, or `new X()` with no arguments. */
function isWrittenInPlace(node) {
  if (node.type === 'ObjectExpression') return true;
  if (node.type === 'ArrayExpression') return node.elements.length === 0;
  return node.type === 'NewExpression' && node.arguments.length === 0;
}

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

/** How deep into type arguments and union members an assertion is compared; real assertions are a level or two deep, and recursive types end here. */
const MAX_DEPTH = 6;
const NULLISH = ts.TypeFlags.Null | ts.TypeFlags.Undefined;

const isTuple = (type) => Boolean((type.target?.objectFlags ?? 0) & ts.ObjectFlags.Tuple);

/** The member of `to` that `from` is asserted into: the one built from the same generic, or else the only one that is not null or undefined. */
function counterpart(from, to) {
  if (!to.isUnion()) return to;
  const members = to.types.filter((member) => (member.flags & NULLISH) === 0);
  const same = members.find(
    (member) => member.target !== undefined && member.target === from.target,
  );
  return same ?? (members.length === 1 ? members[0] : null);
}

/** Whether unchecked type arguments of `from` become checked ones in `to`, position by position, or an array's elements become a tuple's. */
function argumentsClaim(checker, from, to, depth) {
  const fromArguments = typeArguments(checker, from);
  if (fromArguments.length === 0) return false;
  const toArguments = typeArguments(checker, to);
  if (isTuple(to) && !isTuple(from) && fromArguments.length === 1) {
    return toArguments.some((element) =>
      rulesUncheckedShape(checker, fromArguments[0], element, depth + 1),
    );
  }
  if (fromArguments.length !== toArguments.length) return false;
  return fromArguments.some((argument, i) =>
    rulesUncheckedShape(checker, argument, toArguments[i], depth + 1),
  );
}

/** Whether a property of `from` whose type is unchecked has a checked type in `to` (`{ data: unknown }` to `{ data: Message }`). */
function propertiesClaim(checker, from, to) {
  if ((from.flags & ts.TypeFlags.Object) === 0 || (to.flags & ts.TypeFlags.Object) === 0)
    return false;
  return checker.getPropertiesOfType(from).some((property) => {
    const target = checker.getPropertyOfType(to, property.getName());
    if (target === undefined || !isUnchecked(checker, checker.getTypeOfSymbol(property)))
      return false;
    return !claimsNothing(checker, checker.getTypeOfSymbol(target));
  });
}

/** Whether asserting `from` to `to` claims a shape nothing checked: at the top, member by member of a union, inside the type arguments of a container (`unknown[]` to `Message[]`), or in a property (`{ data: unknown }` to `{ data: Message }`). */
function rulesUncheckedShape(checker, from, to, depth = 0) {
  if (depth > MAX_DEPTH) return false;
  if (isUnchecked(checker, from)) return !claimsNothing(checker, to);
  if (from.isUnion()) {
    return from.types.some((member) => {
      const target = counterpart(member, to);
      return target !== null && rulesUncheckedShape(checker, member, target, depth + 1);
    });
  }
  const target = counterpart(from, to);
  if (target === null) return false;
  return argumentsClaim(checker, from, target, depth) || propertiesClaim(checker, from, target);
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
      if (isConstAssertion(node) || isWrittenInPlace(node.expression)) return;
      const type = typeOf(node.expression);
      if (rulesUncheckedShape(checker, type, typeOf(node.typeAnnotation))) report(node, type);
    }

    return {
      TSAsExpression: checkAssertion,
      TSTypeAssertion: checkAssertion,
      TSNonNullExpression: checkNonNull,
    };
  },
};
