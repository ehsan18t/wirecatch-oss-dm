// What an expression in a Svelte file may evaluate to, as far as the file itself says: names are followed to what they were given through the scopes the parser built (so a name that shadows another is told apart from it), runes to their value, members to the property they read, and `{#each}` items to the elements of a list written in the file.

/** Runes whose value is their first argument, and those whose value is what their function returns. */
const VALUE_RUNES = new Set(['$derived', '$state', '$state.raw']);
const RETURNING_RUNES = new Set(['$derived.by']);

/** The variable `identifier` refers to, or null. */
function variableOf(sourceCode, identifier) {
  for (let scope = sourceCode.getScope(identifier); scope; scope = scope.upper) {
    const variable = scope.set.get(identifier.name);
    if (variable) return variable;
  }
  return null;
}

/** What a declaration gives its name: an import, a value, or an item of an `{#each}`; null for a parameter, a prop or anything else. */
function declared(def) {
  if (def.type === 'ImportBinding')
    return { kind: 'import', key: def.node, declaration: def.parent, specifier: def.node };
  if (def.type === 'Variable' && def.node.init)
    return { kind: 'value', key: def.node, node: def.node.init };
  if (def.node.type === 'SvelteEachBlock')
    return { kind: 'each', key: def.name, block: def.node, binding: def.name };
  return null;
}

/** A rune call's name: `$derived`, `$derived.by`, `$state.raw`; null for any other call. */
function runeName(call) {
  const { callee } = call;
  if (callee.type === 'Identifier') return callee.name;
  if (
    callee.type === 'MemberExpression' &&
    callee.object.type === 'Identifier' &&
    callee.property.type === 'Identifier'
  ) {
    return `${callee.object.name}.${callee.property.name}`;
  }
  return null;
}

/** The expressions a function returns: its expression body, or the `return`s of its block outside nested functions. */
function returnedValues(fn) {
  if (fn?.type !== 'ArrowFunctionExpression' && fn?.type !== 'FunctionExpression') return [];
  if (fn.body.type !== 'BlockStatement') return [fn.body];
  const found = [];
  const walk = (node) => {
    if (!node || typeof node.type !== 'string' || /Function/.test(node.type)) return;
    if (node.type === 'ReturnStatement' && node.argument) found.push(node.argument);
    for (const [key, child] of Object.entries(node)) {
      if (key === 'parent') continue;
      for (const item of Array.isArray(child) ? child : [child]) walk(item);
    }
  };
  fn.body.body.forEach(walk);
  return found;
}

function propertyKey(key) {
  if (key.type === 'Identifier') return key.name;
  return key.type === 'Literal' ? String(key.value) : null;
}

/** The keys from an `{#each}` item's pattern down to `binding`: none for `as I`, `['view']` for `as { view: V }`; null when it is not reached through properties. */
function pathTo(pattern, binding) {
  if (pattern === binding) return [];
  const inner = pattern.type === 'AssignmentPattern' ? pattern.left : pattern;
  if (inner === binding) return [];
  if (inner.type !== 'ObjectPattern') return null;
  for (const property of inner.properties) {
    if (property.type !== 'Property' || property.computed) continue;
    const rest = pathTo(property.value, binding);
    if (rest !== null) return [propertyKey(property.key), ...rest];
  }
  return null;
}

/** Follows names, runes, members and `{#each}` items to what they may evaluate to. */
export class SvelteValues {
  constructor(sourceCode) {
    this.sourceCode = sourceCode;
  }

  /** What `expression` may evaluate to: other expressions, or `{ kind: 'import', ... }` for an import. Each declaration is followed once per question, so names declared from each other end. */
  of(expression, seen = new Set()) {
    if (!expression) return [];
    switch (expression.type) {
      case 'Identifier':
        return this.ofName(expression, seen);
      case 'CallExpression':
        return this.ofRune(expression, seen);
      case 'MemberExpression':
        return this.ofMember(expression, seen);
      default:
        return [expression];
    }
  }

  ofName(identifier, seen) {
    const def = variableOf(this.sourceCode, identifier)?.defs[0];
    const found = def === undefined ? null : declared(def);
    if (found === null || seen.has(found.key)) return [];
    seen.add(found.key);
    if (found.kind === 'import') return [found];
    if (found.kind === 'value') return this.of(found.node, seen);
    return this.ofEachItem(found, seen);
  }

  ofRune(call, seen) {
    const rune = runeName(call);
    if (VALUE_RUNES.has(rune)) return this.of(call.arguments[0], seen);
    if (RETURNING_RUNES.has(rune))
      return returnedValues(call.arguments[0]).flatMap((value) => this.of(value, seen));
    return [];
  }

  /** `object.key`: the value of that property where the object is written in the file; an import (`L.X`) stands for its members. A computed key may be any property. */
  ofMember(member, seen) {
    const key = member.computed ? null : propertyKey(member.property);
    return this.of(member.object, seen).flatMap((value) => {
      if (value.kind === 'import') return [value];
      if (value.type !== 'ObjectExpression') return [];
      const properties = value.properties.filter(
        (p) => p.type === 'Property' && (key === null || propertyKey(p.key) === key),
      );
      return properties.flatMap((p) => this.of(p.value, seen));
    });
  }

  /** An `{#each}` item: the matching part of each element of the list, when the list is written in the file. */
  ofEachItem({ block, binding }, seen) {
    const path = pathTo(block.context, binding);
    if (path === null) return [];
    const elements = this.of(block.expression, seen).flatMap((list) =>
      list.type === 'ArrayExpression' ? list.elements.filter(Boolean) : [],
    );
    return elements.flatMap((element) => this.atPath(element, path, seen));
  }

  atPath(expression, path, seen) {
    if (path.length === 0) return this.of(expression, seen);
    const [key, ...rest] = path;
    return this.of(expression, seen).flatMap((value) => {
      if (value.type !== 'ObjectExpression') return [];
      const property = value.properties.find(
        (p) => p.type === 'Property' && !p.computed && propertyKey(p.key) === key,
      );
      return property ? this.atPath(property.value, rest, seen) : [];
    });
  }
}
