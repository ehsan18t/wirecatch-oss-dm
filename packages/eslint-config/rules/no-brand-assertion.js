// No type assertion to a branded id outside the module of its constructors.
//
// The protocol's ids are branded (`number & { readonly __brand: "QueueId" }`) so that one cannot pass for another. `3 as QueueId`, `<QueueId>raw` or `[1, 2] as DownloadId[]` makes an id out of any number without saying which, which is the mistake the brand exists to prevent, and a number written that way also escapes `wirecatch/no-magic-numbers`, which judges `queueId(3)`. An id comes from the app, from a generated guard (`isQueueId`), or from its constructor (`queueId(3)`); the constructors' own module, named by the `constructors` option (ends of paths), is the one place a number is asserted into an id.
//
// A hand-written type predicate (`(v: unknown): v is QueueId => ...`) asserts in the same way, so it is reported too, as is one that narrows to a type the protocol generates (`v is Queue`), which the generated guards check whole: that type is named by the `generated` option (parts of paths). And since a guard narrows whatever it is given, a call of an id's guard on a value that already holds another id (`isQueueId(download.id)`) passes one id off as another, and is reported.
//
// The asserted type is searched where an id can hide in what is written at the assertion: the type itself, the members of a union, the items of a list, and the properties of an object type written in place (`{ id: DownloadId }`). An assertion to a named type that holds ids (`({ ...queue, id: 5 }) as Queue`) narrows it, which `@typescript-eslint/no-unsafe-type-assertion` reports.
import ts from 'typescript';

/** The property a branded type carries only for the compiler. */
const BRAND = '__brand';
/** How deep into unions, lists and written object types the search goes. */
const MAX_DEPTH = 4;

/** Whether `type` is a branded type itself. */
function isBranded(checker, type) {
  return !type.isUnion() && checker.getPropertyOfType(type, BRAND) !== undefined;
}

/** The type arguments of a generic type such as `T[]` or `readonly T[]`, or none. */
function typeArguments(checker, type) {
  return (type.objectFlags ?? 0) & ts.ObjectFlags.Reference ? checker.getTypeArguments(type) : [];
}

/** Whether `type` is an object type written in place, not one with a name. */
function isWrittenObject(type) {
  return (
    (type.flags & ts.TypeFlags.Object) !== 0 &&
    ((type.objectFlags ?? 0) & ts.ObjectFlags.Anonymous) !== 0
  );
}

/** The branded type an assertion to `type` makes, or null. */
function brandIn(checker, type, depth = 0) {
  if (depth > MAX_DEPTH) return null;
  if (isBranded(checker, type)) return type;
  const inner = [
    ...(type.isUnion() ? type.types : []),
    ...typeArguments(checker, type),
    ...(isWrittenObject(type)
      ? checker.getPropertiesOfType(type).map((p) => checker.getTypeOfSymbol(p))
      : []),
  ];
  for (const member of inner) {
    const found = brandIn(checker, member, depth + 1);
    if (found !== null) return found;
  }
  return null;
}

/** Whether an assertion is `as const` (or `<const>`), which only fixes the type of what is written there. */
function isConstAssertion(node) {
  const type = node.typeAnnotation;
  return (
    type.type === 'TSTypeReference' &&
    type.typeName.type === 'Identifier' &&
    type.typeName.name === 'const'
  );
}

/** Whether `type` is declared, or named, in a file whose path holds one of `folders`. */
function declaredIn(type, folders) {
  const symbol = type.aliasSymbol ?? type.getSymbol();
  return (symbol?.getDeclarations() ?? []).some((declaration) => {
    const file = declaration.getSourceFile().fileName;
    return folders.some((folder) => file.includes(folder));
  });
}

/** The generated type a predicate to `type` claims, searched where one can hide as `brandIn` searches for an id (`Queue | null`, `Queue[]`), or null. */
function generatedIn(checker, type, folders, depth = 0) {
  if (depth > MAX_DEPTH) return null;
  if (declaredIn(type, folders)) return type;
  const inner = [...(type.isUnion() ? type.types : []), ...typeArguments(checker, type)];
  for (const member of inner) {
    const found = generatedIn(checker, member, folders, depth + 1);
    if (found !== null) return found;
  }
  return null;
}

/** The functions with a body, whose return type may be a type predicate they make up. */
const FUNCTIONS = ['FunctionDeclaration', 'FunctionExpression', 'ArrowFunctionExpression'];

/** @type {import('eslint').Rule.RuleModule} */
export const noBrandAssertion = {
  meta: {
    type: 'problem',
    docs: {
      description:
        'Disallow `as` and `<T>` assertions to a branded id outside the module of its constructors.',
    },
    schema: [
      {
        type: 'object',
        properties: {
          constructors: { type: 'array', items: { type: 'string' } },
          generated: { type: 'array', items: { type: 'string' } },
        },
        additionalProperties: false,
      },
    ],
    messages: {
      brand:
        '`{{type}}` is a branded id: take it from the app, check a number with its generated guard, or build it with its constructor, instead of asserting it.',
      predicate:
        'A hand-written guard for `{{type}}` claims what the generated guard checks: use the generated guard instead.',
      otherId:
        'This value is already a `{{held}}`; checking it as a `{{type}}` would pass one id off as another.',
    },
  },
  create(context) {
    const constructors = context.options[0]?.constructors ?? [];
    const generated = context.options[0]?.generated ?? [];
    const file = context.filename.replaceAll('\\', '/');
    if (constructors.some((end) => file.endsWith(end))) return {};
    const services = context.sourceCode.parserServices;
    if (!services?.program) {
      throw new Error(
        'wirecatch/no-brand-assertion needs type information (parserOptions.projectService).',
      );
    }
    const checker = services.program.getTypeChecker();

    function check(node) {
      if (isConstAssertion(node)) return;
      const asserted = checker.getTypeAtLocation(
        services.esTreeNodeToTSNodeMap.get(node.typeAnnotation),
      );
      const brand = brandIn(checker, asserted);
      if (brand !== null) {
        context.report({ node, messageId: 'brand', data: { type: checker.typeToString(brand) } });
      }
    }

    function typeOf(node) {
      return checker.getTypeAtLocation(services.esTreeNodeToTSNodeMap.get(node));
    }

    function checkPredicate(node) {
      const predicate = node.returnType?.typeAnnotation;
      if (predicate?.type !== 'TSTypePredicate' || !predicate.typeAnnotation) return;
      const claimed = typeOf(predicate.typeAnnotation.typeAnnotation);
      const brand = brandIn(checker, claimed);
      const shown = brand ?? generatedIn(checker, claimed, generated);
      if (shown !== null) {
        context.report({
          node: predicate,
          messageId: 'predicate',
          data: { type: checker.typeToString(shown) },
        });
      }
    }

    function checkGuardCall(node) {
      const signature = checker.getResolvedSignature(services.esTreeNodeToTSNodeMap.get(node));
      const predicate = signature && checker.getTypePredicateOfSignature(signature);
      const argument =
        predicate?.parameterIndex === undefined
          ? undefined
          : node.arguments[predicate.parameterIndex];
      if (!predicate?.type || argument === undefined) return;
      const brand = brandIn(checker, predicate.type);
      const held = brandIn(checker, typeOf(argument));
      if (brand === null || held === null) return;
      const [type, heldType] = [checker.typeToString(brand), checker.typeToString(held)];
      if (type !== heldType) {
        context.report({ node, messageId: 'otherId', data: { type, held: heldType } });
      }
    }

    return {
      TSAsExpression: check,
      TSTypeAssertion: check,
      CallExpression: checkGuardCall,
      ...Object.fromEntries(FUNCTIONS.map((kind) => [kind, checkPredicate])),
    };
  },
};
