// Writes validators.ts: a type guard for every type ts-rs generated, built from the generated declarations themselves, so a guard can never fall behind its type. Each guard checks the whole shape (every field, every union member, every list item) and accepts fields it does not know, so a newer app's additions still read.
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import ts from 'typescript';

const EXTENSION = '.ts';
/** The file this writes, which index-source.mjs re-exports by this name. */
export const VALIDATORS = 'validators';
/** Files in the folder that are not one ts-rs type each. */
const NOT_TYPES = new Set(['constants', VALIDATORS]);

/** Why a declaration gets no guard: a shape the generator cannot check. */
class Unsupported extends Error {}

/** The generated type declarations in `folder`, by name. */
function declarations(folder) {
  const found = new Map();
  for (const file of readdirSync(folder)) {
    const name = file.slice(0, -EXTENSION.length);
    if (!file.endsWith(EXTENSION) || NOT_TYPES.has(name)) continue;
    const text = readFileSync(join(folder, file), 'utf8');
    const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
    for (const statement of source.statements) {
      if (ts.isTypeAliasDeclaration(statement)) found.set(statement.name.text, statement);
    }
  }
  return found;
}

/** The guard function's name for the type `name`. */
export function guardName(name) {
  return `is${name}`;
}

/** The name of the parameter checked at `depth`: nested object types each get their own. */
function parameter(depth) {
  return depth === 0 ? 'value' : `value${String(depth)}`;
}

/** A property access that also works under `noPropertyAccessFromIndexSignature`. */
function property(subject, key) {
  return `${subject}[${JSON.stringify(key)}]`;
}

/** The check that `subject` holds the literal `node`. */
function literalCheck(node, subject) {
  const { literal } = node;
  if (literal.kind === ts.SyntaxKind.NullKeyword) return `${subject} === null`;
  if (ts.isStringLiteral(literal)) return `${subject} === ${JSON.stringify(literal.text)}`;
  if (literal.kind === ts.SyntaxKind.TrueKeyword) return `${subject} === true`;
  if (literal.kind === ts.SyntaxKind.FalseKeyword) return `${subject} === false`;
  if (ts.isNumericLiteral(literal)) return `${subject} === ${literal.text}`;
  throw new Unsupported(`literal ${literal.getText()}`);
}

/** The guard of the generated type `node` names. */
function namedGuard(node, context) {
  const name = node.typeName.getText();
  if (node.typeArguments !== undefined || !context.known.has(name)) {
    throw new Unsupported(`type ${node.getText()}`);
  }
  return guardName(name);
}

/** Whether `node` names a list. */
function isList(node) {
  return node.typeName.getText() === 'Array' && node.typeArguments?.length === 1;
}

/** The check that `subject` is of the referenced type: a list, a type parameter, a generic type with its arguments, or another generated type. */
function referenceCheck(node, subject, context) {
  if (!isList(node)) {
    const name = node.typeName.getText();
    const bound = context.parameters.get(name);
    if (bound !== undefined)
      return check(bound.node, subject, { ...bound.context, depth: context.depth });
    const generic = context.found.get(name);
    if (
      node.typeArguments !== undefined &&
      generic?.typeParameters?.length === node.typeArguments.length
    ) {
      const parameters = new Map(
        generic.typeParameters.map((typeParameter, i) => [
          typeParameter.name.text,
          { node: node.typeArguments[i], context },
        ]),
      );
      return `(${check(generic.type, subject, { ...context, parameters })})`;
    }
    return `${namedGuard(node, context)}(${subject})`;
  }
  const [itemType] = node.typeArguments;
  // A list of a generated type passes that type's guard as it is.
  if (
    ts.isTypeReferenceNode(itemType) &&
    !isList(itemType) &&
    itemType.typeArguments === undefined
  ) {
    return `isArrayOf(${subject}, ${namedGuard(itemType, context)})`;
  }
  const item = context.depth + 1;
  const inner = check(itemType, parameter(item), { ...context, depth: item });
  return `isArrayOf(${subject}, (${parameter(item)}: unknown) => ${inner})`;
}

/** The check that `subject` is an object with these members; fields the type does not name are not checked. */
function objectCheck(node, subject, context) {
  const depth = context.depth + 1;
  const inner = parameter(depth);
  const fields = node.members.map((member) => {
    if (!ts.isPropertySignature(member) || member.type === undefined) {
      throw new Unsupported(`member ${member.getText()}`);
    }
    const key = ts.isStringLiteral(member.name) ? member.name.text : member.name.getText();
    const at = property(inner, key);
    const fieldCheck = check(member.type, at, { ...context, depth });
    return member.questionToken === undefined
      ? `(${fieldCheck})`
      : `(${at} === undefined || ${fieldCheck})`;
  });
  const body = [`isRecord(${inner})`, ...fields].join(' && ');
  return `((${inner}: unknown): boolean => ${body})(${subject})`;
}

/** The property a branded id carries only in its type (`number & { readonly __brand: "DownloadId" }`), never at run time. */
const BRAND = '__brand';

/** Whether `node` is the brand part of a branded type, which exists only for the compiler. */
function isBrand(node) {
  return (
    ts.isTypeLiteralNode(node) &&
    node.members.length > 0 &&
    node.members.every((m) => ts.isPropertySignature(m) && m.name.getText() === BRAND)
  );
}

/** The check that `subject` is every member of the intersection `node` but its brand. A branded number is an id, which the app counts from 0 up, so it is a whole number no smaller than 0 that JavaScript holds exactly. */
function intersectionCheck(node, subject, context) {
  const checked = node.types.filter((member) => !isBrand(member));
  if (checked.length === 0) throw new Unsupported(`type ${node.getText()}`);
  const branded = checked.length < node.types.length;
  return checked
    .map((member) =>
      branded && member.kind === ts.SyntaxKind.NumberKeyword
        ? `(typeof ${subject} === "number" && Number.isSafeInteger(${subject}) && ${subject} >= 0)`
        : `(${check(member, subject, context)})`,
    )
    .join(' && ');
}

/** The boolean expression that checks `subject` against the type `node`. */
function check(node, subject, context) {
  switch (node.kind) {
    case ts.SyntaxKind.StringKeyword:
      return `typeof ${subject} === "string"`;
    case ts.SyntaxKind.NumberKeyword:
      return `typeof ${subject} === "number"`;
    case ts.SyntaxKind.BooleanKeyword:
      return `typeof ${subject} === "boolean"`;
    case ts.SyntaxKind.NullKeyword:
      return `${subject} === null`;
    default:
      break;
  }
  if (ts.isParenthesizedTypeNode(node)) return check(node.type, subject, context);
  if (ts.isLiteralTypeNode(node)) return literalCheck(node, subject);
  if (ts.isUnionTypeNode(node)) {
    return node.types.map((member) => `(${check(member, subject, context)})`).join(' || ');
  }
  if (ts.isIntersectionTypeNode(node)) return intersectionCheck(node, subject, context);
  if (ts.isTypeReferenceNode(node)) return referenceCheck(node, subject, context);
  if (ts.isTypeLiteralNode(node)) return objectCheck(node, subject, context);
  throw new Unsupported(`type ${node.getText()}`);
}

/** What every check reads: the declarations, the types that have a guard, and the type arguments in force inside a generic type. */
function rootContext(found, known) {
  return { depth: 0, found, known, parameters: new Map() };
}

/** A guard for each declaration that can have one; a type that names one without a guard gets none either. A generic type has none of its own: it is checked where it is used, with its type arguments. */
function guards(found) {
  const known = new Set([...found.keys()].filter((name) => !found.get(name).typeParameters));
  const written = new Map();
  let changed = true;
  while (changed) {
    changed = false;
    for (const name of [...known].sort()) {
      try {
        written.set(name, check(found.get(name).type, parameter(0), rootContext(found, known)));
      } catch (error) {
        if (!(error instanceof Unsupported)) throw error;
        known.delete(name);
        written.delete(name);
        changed = true;
      }
    }
  }
  return written;
}

/** The generic type the command map wraps each command's arguments and answer in (`desktop.rs`). */
const COMMAND = 'Command';
/** The command map: one `Command<Args, Output>` per command. */
const COMMAND_MAP = 'CommandMap';

/** A guard for the answer of every command in the command map, or nothing when there is none. */
function commandOutputs(found, known) {
  const map = found.get(COMMAND_MAP);
  if (map === undefined || !ts.isTypeLiteralNode(map.type)) return [];
  const entries = map.type.members.map((member) => {
    const command = member.type;
    const named = command !== undefined && ts.isTypeReferenceNode(command);
    if (!named || command.typeName.getText() !== COMMAND || command.typeArguments?.length !== 2) {
      throw new Error(`${COMMAND_MAP}.${member.name.getText()} is not a ${COMMAND}<Args, Output>`);
    }
    const name = member.name.getText();
    const answer = `${COMMAND_MAP}["${name}"]["output"]`;
    const body = check(command.typeArguments[1], parameter(0), rootContext(found, known));
    return `  ${name}: (value: unknown): value is ${answer} => ${body},`;
  });
  return [
    '',
    "/** A guard for each command's answer, so what the shell sends back is checked before it is used. */",
    `export const COMMAND_OUTPUTS: { readonly [C in keyof ${COMMAND_MAP}]: (value: unknown) => value is ${COMMAND_MAP}[C]["output"] } = {`,
    ...entries,
    '};',
  ];
}

/** The generated type that has no guard on purpose: the command map, checked one answer at a time by `COMMAND_OUTPUTS`. */
const WITHOUT_GUARD = [COMMAND_MAP];

/** The text of validators.ts for the generated types in `folder`. Fails when a type other than those in `withoutGuard` gets no guard, so a type the generator cannot check (a map, a generic) is never left unchecked without anyone noticing. */
export function validatorsSource(folder, { withoutGuard = WITHOUT_GUARD } = {}) {
  const found = declarations(folder);
  const written = guards(found);
  const unguarded = [...found.keys()].filter(
    (name) => !written.has(name) && !withoutGuard.includes(name) && !found.get(name).typeParameters,
  );
  if (unguarded.length > 0) {
    throw new Error(
      `No guard could be generated for ${unguarded.sort().join(', ')}: give each a shape the generator can check, or list it in WITHOUT_GUARD with the reason.`,
    );
  }
  const outputs = commandOutputs(found, new Set(written.keys()));
  const names = [...written.keys()].sort();
  const lines = [
    '// Generated by packages/protocol/scripts/validators.mjs from the types ts-rs wrote. Do not edit; run `pnpm protocol:generate`.',
    '',
    ...names.map((name) => `import type { ${name} } from "./${name}";`),
    ...(outputs.length > 0 ? [`import type { ${COMMAND_MAP} } from "./${COMMAND_MAP}";`] : []),
    '',
    'function isRecord(value: unknown): value is Record<string, unknown> {',
    '  return typeof value === "object" && value !== null && !Array.isArray(value);',
    '}',
    '',
    'function isArrayOf(value: unknown, item: (value: unknown) => boolean): boolean {',
    '  return Array.isArray(value) && value.every(item);',
    '}',
  ];
  for (const name of names) {
    lines.push(
      '',
      `/** Whether \`value\` has the shape of a {@link ${name}}; fields it does not name are not checked. */`,
      `export function ${guardName(name)}(value: unknown): value is ${name} {`,
      `  return ${written.get(name)};`,
      '}',
    );
  }
  lines.push(...outputs, '');
  return lines.join('\n');
}
