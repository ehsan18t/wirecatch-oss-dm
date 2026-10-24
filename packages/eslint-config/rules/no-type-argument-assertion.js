// No type argument that only asserts what a call returns.
//
// A type argument given to a generic function is checked against its arguments when the type parameter takes part in them: `structuredClone<Settings>(value)` fails to compile unless `value` is `Settings`. When it takes no part, nothing is checked, and the argument only says what the result is: `invoke<Settings>('get_settings')`, `listen<UiEvent>(name, (e) => use(e.payload))` or `querySelector<HTMLInputElement>(selector)` do what `as` does, with no `as` for `wirecatch/no-assertion-on-input` to see. Such a call takes no type argument; its result is checked with a guard instead.
//
// A call given no arguments has nothing to check, so its type argument asserts too, except for the calls named by the `declarers` option, whose type argument declares what will be put there (`$state<HTMLDivElement>()`, `createContext<AppState>()`), as an annotation would.
//
// The same claim can be made without a type argument, by annotating a parameter of a callback the call is given: with `listen<T>(name, handler: (event: Event<T>) => void)`, `listen(name, (e: Event<UiEvent>) => ...)` infers `T` from the annotation alone, and a callback whose parameter is given `any` (a browser's message listener) takes any annotation. Both are reported; `unknown` is always allowed.
//
// A type parameter takes part when it appears in the type of a parameter as something the caller hands over: the parameter itself, an item of a list, a field, or what a callback the caller passes returns. Inside the parameters of such a callback, the function hands the value to the caller, so it takes no part there (`handler: (event: Event<T>) => void`); a callback that takes a callback hands it back, so it does again (the `resolve` of `new Promise<T>`).
import ts from 'typescript';

/** How deep into the types of parameters the search goes. */
const MAX_DEPTH = 6;

/** The type arguments of a generic type such as `T[]` or `Promise<T>`, or none. */
function typeArguments(checker, type) {
  return (type.objectFlags ?? 0) & ts.ObjectFlags.Reference ? checker.getTypeArguments(type) : [];
}

/** Whether `type` is an object type written in place, whose fields are part of it. */
function isWrittenObject(type) {
  return (
    (type.flags & ts.TypeFlags.Object) !== 0 &&
    ((type.objectFlags ?? 0) & ts.ObjectFlags.Anonymous) !== 0
  );
}

/** Whether `type` is one the search cannot see into (a mapped or conditional type), where the type parameter is taken to take part, so nothing is reported that might be checked. */
function isOpaque(type) {
  return (
    (type.flags & ts.TypeFlags.Conditional) !== 0 ||
    ((type.objectFlags ?? 0) & ts.ObjectFlags.Mapped) !== 0
  );
}

/** What a signature hands back: its return type, or the type its predicate (`value is T`) narrows to. */
function returned(checker, signature) {
  return checker.getTypePredicateOfSignature(signature)?.type ?? signature.getReturnType();
}

/** The types inside `type` the search goes on into, each with whether it is handed the other way, as the parameters of a callback are. */
function innerTypes(checker, type) {
  if (type.isUnionOrIntersection()) return type.types.map((member) => [member, false]);
  if ((type.flags & ts.TypeFlags.IndexedAccess) !== 0) return [[type.objectType, false]];
  const signatures = type
    .getCallSignatures()
    .flatMap((signature) => [
      ...signature.getParameters().map((p) => [checker.getTypeOfSymbol(p), true]),
      [returned(checker, signature), false],
    ]);
  const fields = isWrittenObject(type)
    ? checker.getPropertiesOfType(type).map((p) => [checker.getTypeOfSymbol(p), false])
    : [];
  return [...signatures, ...typeArguments(checker, type).map((t) => [t, false]), ...fields];
}

/** Whether `search.target` appears in `type` as something the caller hands over (`handed`), or as something handed back to the caller when `handed` is false. */
function takesPart(search, type, handed, depth = 0) {
  if (type === search.target) return handed;
  if (depth > MAX_DEPTH) return false;
  const next = ([inner, flipped]) =>
    takesPart(search, inner, flipped ? !handed : handed, depth + 1);
  // A named one (`Record<string, T>`) is judged by what it is given.
  if (isOpaque(type)) {
    return type.aliasTypeArguments?.some((member) => next([member, false])) ?? true;
  }
  return innerTypes(search.checker, type).some(next);
}

/** Calls given no arguments whose type argument only declares what will be put there, as an annotation would (`$state<HTMLDivElement>()`). */
const DECLARERS = ['$state', '$state.raw', 'createContext', 'writable'];

/** @type {import('eslint').Rule.RuleModule} */
export const noTypeArgumentAssertion = {
  meta: {
    type: 'problem',
    docs: {
      description:
        'Disallow a type argument, or a callback parameter annotation, that only says what a call hands back, which asserts it as `as` would.',
    },
    schema: [
      {
        type: 'object',
        properties: { declarers: { type: 'array', items: { type: 'string' } } },
        additionalProperties: false,
      },
    ],
    messages: {
      assertion:
        '`{{name}}` takes no part in what this call is given, so `{{argument}}` only asserts what it returns: leave the type argument out and check the result with a guard.',
      annotation:
        'This annotation says what `{{name}}` is, which nothing this call is given checks: annotate the parameter `unknown` and check it with a guard.',
      anyParameter:
        'This parameter is given `any`, so its annotation asserts what nothing checked: annotate it `unknown` and check it with a guard.',
    },
  },
  create(context) {
    const services = context.sourceCode.parserServices;
    if (!services?.program) {
      throw new Error(
        'wirecatch/no-type-argument-assertion needs type information (parserOptions.projectService).',
      );
    }
    const checker = services.program.getTypeChecker();
    const declarers = context.options[0]?.declarers ?? DECLARERS;
    const typeOf = (node) => checker.getTypeAtLocation(services.esTreeNodeToTSNodeMap.get(node));

    /** The type parameters of the signature `node` calls, and the types of its parameters as declared. */
    function signatureOf(node) {
      const declaration = checker
        .getResolvedSignature(services.esTreeNodeToTSNodeMap.get(node))
        ?.getDeclaration();
      return {
        typeParameters: declaration?.typeParameters ?? [],
        parameters: (declaration?.parameters ?? []).map((p) => checker.getTypeAtLocation(p)),
      };
    }

    /** Whether the type parameter `target` takes part in nothing the call is given. */
    function unchecked(target, parameters) {
      return parameters.every((type) => !takesPart({ checker, target }, type, true));
    }

    function checkTypeArguments(node, { typeParameters, parameters }) {
      const written = node.typeArguments?.params ?? [];
      const declares =
        node.arguments.length === 0 && declarers.includes(context.sourceCode.getText(node.callee));
      if (declares) return;
      written.forEach((argument, index) => {
        const parameter = typeParameters[index];
        if (parameter === undefined || !unchecked(checker.getTypeAtLocation(parameter), parameters))
          return;
        context.report({
          node: argument,
          messageId: 'assertion',
          data: { name: parameter.name.text, argument: context.sourceCode.getText(argument) },
        });
      });
    }

    /** Reports an annotated parameter of a callback the call is given, where the annotation is all that says what an unchecked type parameter, or `any`, is. */
    function checkCallback(callback, declared, free) {
      const signature = checker.getNonNullableType(declared).getCallSignatures()[0];
      if (signature === undefined) return;
      callback.params.forEach((param, index) => {
        const expected = signature.getParameters()[index];
        if (!param.typeAnnotation || expected === undefined) return;
        const given = checker.getTypeOfSymbol(expected);
        const said = typeOf(param.typeAnnotation.typeAnnotation);
        const open = ts.TypeFlags.Any | ts.TypeFlags.Unknown;
        if ((given.flags & ts.TypeFlags.Any) !== 0 && (said.flags & open) === 0) {
          context.report({ node: param, messageId: 'anyParameter' });
          return;
        }
        const named = free.find((parameter) =>
          takesPart({ checker, target: checker.getTypeAtLocation(parameter) }, given, true),
        );
        if (named !== undefined) {
          context.report({ node: param, messageId: 'annotation', data: { name: named.name.text } });
        }
      });
    }

    function checkCallbacks(node, { typeParameters, parameters }) {
      if ((node.typeArguments?.params.length ?? 0) > 0) return;
      const free = typeParameters.filter((parameter) =>
        unchecked(checker.getTypeAtLocation(parameter), parameters),
      );
      node.arguments.forEach((argument, index) => {
        const callback =
          argument.type === 'ArrowFunctionExpression' || argument.type === 'FunctionExpression';
        if (callback && parameters[index] !== undefined) {
          checkCallback(argument, parameters[index], free);
        }
      });
    }

    function check(node) {
      const signature = signatureOf(node);
      checkTypeArguments(node, signature);
      checkCallbacks(node, signature);
    }

    return { CallExpression: check, NewExpression: check };
  },
};
