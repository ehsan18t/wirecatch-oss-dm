// Keeps raw values out of components built on the design system, so every colour, size and step has one home: the tokens (D36, docs/ENGINEERING.md section 2).
//
// A package turns it on for its design-system code (packages/ui, and each app's `src/ui/` as screens move there). It reads every string in TypeScript and Svelte, and the markup of Svelte components, and reports:
//
// - a Tailwind arbitrary value or property (`w-[13px]`, `bg-(--brand)`, `[color:red]`); arbitrary variants (`[&_svg]:size-4`) are selectors, not values, and pass;
// - a bare opacity (`opacity-45`) or colour alpha (`bg-danger/14`), which are magic numbers: name the step in tokens.css (`opacity-disabled`, `bg-danger/soft`). `opacity-0`, `opacity-100` and fractions (`w-1/2`) pass;
// - a class named by a `class:` directive, checked like any other class;
// - a colour written out (`#fff`, `rgb(...)`, `oklch(...)`) anywhere in a string;
// - a length or colour (named ones included) written into a style: a `style` attribute or a `style:` directive (`style="padding: 13px"`, `style:width="2rem"`), a string that reads as declarations (`'color: red'`, `'padding:13px'`), or a value given to `element.style.*` or `setProperty`; a style built from a value (`style:--fill="{share}%"`) passes;
// - a `<style>` block: a component styles itself through the token utilities only, and a block (with or without `@apply`) is where raw values would gather unseen;
// - a width or height written on an `<svg>` (or `<svelte:element this="svg">`): an icon takes its size from a class, so it follows the icon scale;
// - a stylesheet imported beside a component (`import './x.css'`), the way around a refused `<style>` block; the design system's own `tokens.css` and `base.css` pass;
// - a raw duration, delay or distance: a number other than 0 or 1 in a `transition:`, `in:`, `out:` or `animate:` directive or in an `element.animate()` call, and a bare `duration-*` or `delay-*` utility (`duration-300`). Motion comes from the motion presets and tokens (`fade`, `shift`, `duration()`, `distance()`, `duration-normal`).
//
// What it cannot see: class names built from pieces at run time, and values that arrive from outside the file.

/** A colour written out: a hex colour on its own, or a CSS colour function. */
const HEX_COLOR = /^#(?:[\da-f]{3,4}|[\da-f]{6}|[\da-f]{8})$/i;
const COLOR_FUNCTION = /\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color-mix)\(/i;
/** A length with a unit, as a style writes it; `0` alone needs no unit and passes. */
const LENGTH = /(?:^|[^\w.-])(?:\d*\.)?\d+(?:px|rem|em|vh|vw|vmin|vmax|ch|pt|%)(?![\w-])/i;
/** CSS's named colours, which a style may write as plainly as a hex value. `transparent`, `currentcolor` and `inherit` name no colour of their own and pass. */
const NAMED_COLORS = new Set(
  [
    'aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue blueviolet brown',
    'burlywood cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson cyan darkblue darkcyan',
    'darkgoldenrod darkgray darkgreen darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid',
    'darkred darksalmon darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet',
    'deeppink deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro',
    'ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo ivory khaki',
    'lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan lightgoldenrodyellow',
    'lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen lightskyblue lightslategray',
    'lightslategrey lightsteelblue lightyellow lime limegreen linen magenta maroon mediumaquamarine',
    'mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen',
    'mediumturquoise mediumvioletred midnightblue mintcream mistyrose moccasin navajowhite navy oldlace',
    'olive olivedrab orange orangered orchid palegoldenrod palegreen paleturquoise palevioletred',
    'papayawhip peachpuff peru pink plum powderblue purple rebeccapurple red rosybrown royalblue',
    'saddlebrown salmon sandybrown seagreen seashell sienna silver skyblue slateblue slategray slategrey',
    'snow springgreen steelblue tan teal thistle tomato turquoise violet wheat white whitesmoke yellow',
    'yellowgreen',
  ]
    .join(' ')
    .split(' '),
);
/** A declaration in a string: a property, a colon, and its value up to the next `;`. */
const DECLARATION = /(?:^|[;{\s])(--[\w-]+|[a-z][a-z-]*)\s*:(\s*)([^;]*)/g;

const ARBITRARY = /-(?:\[[^\]]+\]|\([^)]+\))$/;
const ARBITRARY_PROPERTY = /^\[[\w-]+:[^\]]+\]$/;
const BARE_OPACITY = /^-?opacity-(\d+)$/;
/** A duration or delay written as milliseconds (`duration-300`) instead of a named step (`duration-normal`). */
const BARE_TIMING = /^(?:duration|delay)-\d+$/;
/** Numbers that read as themselves in motion: no offset, full scale or opacity. */
const OBVIOUS_MOTION = new Set([0, 1]);
/** The stylesheets a design-system file may import: the design system's own. */
const ALLOWED_CSS = new Set(['@wirecatch/ui/tokens.css', '@wirecatch/ui/base.css']);
const CSS_MODULE = /\.css(?:\?.*)?$/;
/** An alpha step on a colour utility: the name before the slash ends in a letter, so `w-1/2` is a fraction. */
const BARE_ALPHA = /^[a-z][\w-]*[a-z]\/\d+$/;
/** Opacities that read as themselves. */
const OBVIOUS_OPACITY = new Set(['0', '100']);

/** The utility a class names, after its variants: the part past the last `:` outside brackets. */
function utilityOf(token) {
  let depth = 0;
  let start = 0;
  for (let i = 0; i < token.length; i++) {
    const char = token[i];
    if (char === '[' || char === '(') depth++;
    else if (char === ']' || char === ')') depth--;
    else if (char === ':' && depth === 0) start = i + 1;
  }
  return token.slice(start).replaceAll(/^!|!$/g, '');
}

/** What is wrong with one whitespace-separated piece of a string, or null. */
function classProblem(token) {
  if (HEX_COLOR.test(token.replace(/[;,'"`]+$/, ''))) return 'rawColor';
  const utility = utilityOf(token);
  if (ARBITRARY.test(utility) || ARBITRARY_PROPERTY.test(utility)) return 'arbitraryValue';
  const opacity = BARE_OPACITY.exec(utility);
  if (opacity && !OBVIOUS_OPACITY.has(opacity[1])) return 'rawOpacity';
  if (BARE_ALPHA.test(utility)) return 'rawAlpha';
  if (BARE_TIMING.test(utility)) return 'rawMotion';
  return null;
}

/** Every problem in a string that may hold classes or other text. */
function stringProblems(text) {
  const found = new Set();
  if (COLOR_FUNCTION.test(text)) found.add('rawColor');
  for (const token of text.split(/\s+/)) {
    const problem = token === '' ? null : classProblem(token);
    if (problem) found.add(problem);
  }
  return [...found];
}

/** Every problem in text written into a style. */
function styleProblems(text) {
  const found = [];
  if (LENGTH.test(text)) found.push('rawLength');
  const parts = text.toLowerCase().split(/[\s:;,()/]+/);
  const written =
    COLOR_FUNCTION.test(text) ||
    parts.some((part) => NAMED_COLORS.has(part) || HEX_COLOR.test(part));
  if (written) found.push('rawColor');
  return found;
}

/** Whether a value after a colon starts like a style value (a number, a colour), not like a Tailwind utility after its variant (`hover:bg-wash`). */
const looksLikeStyleValue = (value) =>
  /^[\d.#-]/.test(value) ||
  COLOR_FUNCTION.test(value) ||
  NAMED_COLORS.has(value.split(/\s/)[0].toLowerCase());

/**
 * The values of the declarations a string holds, when it reads as style text: `'color: red'`, `'padding:13px'`, `'--gap: 4px'`. A custom property, a space after the colon, a `;` in the string or a style-like value makes a declaration; a class string's variants (`hover:bg-wash`) do not.
 */
function declarationValues(text) {
  const values = [];
  for (const [, name, space, value] of text.matchAll(DECLARATION)) {
    const trimmed = value.trim();
    const declaration =
      name.startsWith('--') || space !== '' || text.includes(';') || looksLikeStyleValue(trimmed);
    if (declaration) values.push(trimmed);
  }
  return values;
}

/** Problems in a string anywhere in code: as classes, and as style text when it reads as declarations. */
function codeStringProblems(text) {
  const values = declarationValues(text);
  const style = values.length === 0 ? [] : styleProblems(values.join(' '));
  return [...new Set([...stringProblems(text), ...style])];
}

function propertyName(member) {
  if (member.computed) return member.property.type === 'Literal' ? member.property.value : null;
  return member.property.type === 'Identifier' ? member.property.name : null;
}

/** Whether a string literal is a value given straight to an element's style: `el.style.padding = '13px'`, `el.style.cssText = ...`, `style.setProperty('width', '2rem')`. */
function isStyleValue(node) {
  const { parent } = node;
  if (parent.type === 'AssignmentExpression' && parent.right === node) {
    const target = parent.left;
    if (target.type !== 'MemberExpression') return false;
    const onStyle =
      target.object.type === 'MemberExpression' && propertyName(target.object) === 'style';
    return onStyle || propertyName(target) === 'style';
  }
  return (
    parent.type === 'CallExpression' &&
    parent.callee.type === 'MemberExpression' &&
    propertyName(parent.callee) === 'setProperty' &&
    parent.arguments[1] === node
  );
}

const isObjectAssign = (callee) =>
  callee.type === 'MemberExpression' &&
  callee.object.type === 'Identifier' &&
  callee.object.name === 'Object' &&
  propertyName(callee) === 'assign';

const isAnimateCall = (node) =>
  node.type === 'CallExpression' &&
  node.callee.type === 'MemberExpression' &&
  propertyName(node.callee) === 'animate';

/** Whether a string literal is a property value in an object given to an element's style: `Object.assign(el.style, { padding: '13px' })`, or a keyframe of `el.animate([{ translate: '0 8px' }], ...)`. */
function isStyleObjectValue(node) {
  const property = node.parent;
  if (property.type !== 'Property' || property.value !== node) return false;
  let holder = property.parent.parent;
  if (holder?.type === 'ArrayExpression') holder = holder.parent;
  if (holder?.type !== 'CallExpression') return false;
  if (isAnimateCall(holder)) return true;
  const [target] = holder.arguments;
  return (
    isObjectAssign(holder.callee) &&
    target?.type === 'MemberExpression' &&
    propertyName(target) === 'style'
  );
}

/** Whether a number sits in motion: the arguments of a transition or animation directive, or of an `element.animate()` call, short of any function inside them. */
function inMotion(node) {
  for (let ancestor = node.parent; ancestor; ancestor = ancestor.parent) {
    if (ancestor.type === 'SvelteDirective') {
      return ancestor.kind === 'Transition' || ancestor.kind === 'Animation';
    }
    if (isAnimateCall(ancestor)) return true;
    if (/Function|Statement|Declaration|Program|SvelteElement/.test(ancestor.type)) return false;
  }
  return false;
}

/** The literal text in an attribute's or directive's value, and whether a part of it is a literal expression (`style:width={13}`). */
function literalParts(value) {
  return (value ?? []).flatMap((part) => {
    if (part.type === 'SvelteLiteral') return [part.value];
    if (part.type === 'SvelteMustacheTag' && part.expression.type === 'Literal') {
      return [String(part.expression.value)];
    }
    return [];
  });
}

/** Whether `<svelte:element>` is given the literal tag `svg`. */
function rendersSvg(attribute) {
  if (attribute.type === 'SvelteSpecialDirective' && attribute.kind === 'this') {
    return attribute.expression?.type === 'Literal' && attribute.expression.value === 'svg';
  }
  return (
    attribute.type === 'SvelteAttribute' &&
    attribute.key.name === 'this' &&
    literalParts(attribute.value).join('') === 'svg'
  );
}

/** Whether an element renders an `<svg>`: by its tag, or as `<svelte:element this="svg">`. */
function isSvg(element) {
  if (element.type !== 'SvelteElement') return false;
  if (element.name.name === 'svg') return true;
  return element.name.name === 'svelte:element' && element.startTag.attributes.some(rendersSvg);
}

/** @type {import('eslint').Rule.RuleModule} */
export const tokensOnly = {
  meta: {
    type: 'problem',
    docs: {
      description: 'Style design-system components through the tokens only, never raw values.',
    },
    schema: [],
    messages: {
      arbitraryValue:
        'An arbitrary value bypasses the tokens: add the value to tokens.css and use its utility.',
      rawOpacity:
        'A bare opacity is a magic number: name the step in tokens.css (`--opacity-*`) and use it (`opacity-disabled`).',
      rawAlpha:
        'A bare colour alpha is a magic number: name the step in tokens.css (`--opacity-*`) and use it (`bg-danger/soft`).',
      rawColor: 'A colour written out bypasses the tokens: use a colour utility from tokens.css.',
      rawLength:
        'A length written into a style bypasses the tokens: use a utility, or a value computed from state.',
      svgSize:
        'An icon takes its size from a class on the icon scale (`size-icon-sm`), not from width and height.',
      styleBlock:
        'A component styles itself through the token utilities only: move what this <style> block does into classes, and any new value into tokens.css.',
      cssImport:
        'A stylesheet beside a component bypasses the tokens: style through the token utilities, and add any new value to tokens.css.',
      rawMotion:
        'A raw duration, delay or distance bypasses the motion tokens: use a preset from motion.ts (`fade`, `shift`, `duration()`, `distance()`) or a named utility (`duration-normal`).',
    },
  },
  create(context) {
    const report = (node, messageIds) => {
      for (const messageId of messageIds) context.report({ node, messageId });
    };
    return {
      Literal(node) {
        if (typeof node.value === 'number') {
          if (!OBVIOUS_MOTION.has(node.value) && inMotion(node)) report(node, ['rawMotion']);
          return;
        }
        if (typeof node.value !== 'string') return;
        const toStyle = isStyleValue(node) || isStyleObjectValue(node);
        const style = toStyle ? styleProblems(node.value) : [];
        report(node, [...new Set([...codeStringProblems(node.value), ...style])]);
      },
      TemplateElement(node) {
        report(node, codeStringProblems(node.value.cooked ?? node.value.raw));
      },
      SvelteStyleElement(node) {
        report(node, ['styleBlock']);
      },
      // A <style> element anywhere in the markup, such as inside <svelte:head>.
      SvelteElement(node) {
        if (node.kind === 'html' && node.name.name === 'style') report(node, ['styleBlock']);
      },
      ImportDeclaration(node) {
        const source = String(node.source.value);
        if (CSS_MODULE.test(source) && !ALLOWED_CSS.has(source)) report(node, ['cssImport']);
      },
      ImportExpression(node) {
        const { source } = node;
        const css = source.type === 'Literal' && CSS_MODULE.test(String(source.value));
        if (css && !ALLOWED_CSS.has(String(source.value))) report(node, ['cssImport']);
      },
      SvelteDirective(node) {
        if (node.kind !== 'Class') return;
        const problem = classProblem(node.key.name.name);
        if (problem) report(node, [problem]);
      },
      SvelteAttribute(node) {
        const name = node.key.name;
        const element = node.parent.parent;
        if (name === 'style') {
          report(node, styleProblems(literalParts(node.value).join(' ')));
          return;
        }
        const size = name === 'width' || name === 'height';
        if (size && isSvg(element) && literalParts(node.value).length > 0) {
          report(node, ['svgSize']);
          return;
        }
        for (const part of node.value) {
          if (part.type === 'SvelteLiteral') report(part, stringProblems(part.value));
        }
      },
      SvelteStyleDirective(node) {
        report(node, styleProblems(literalParts(node.value).join(' ')));
      },
    };
  },
};
