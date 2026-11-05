// The product's name in text a person reads comes from `@wirecatch/product`, so a rename leaves nothing behind (D175). Names in comments, and in identifiers, ids and keys, are not read.

/** The nodes that hold written text: a script's strings and templates, and a Svelte file's text and attribute values. */
const TEXT_NODES = ['Literal', 'TemplateElement', 'SvelteText', 'SvelteLiteral'];

function textOf(node) {
  if (node.type === 'TemplateElement') return node.value.raw;
  return typeof node.value === 'string' ? node.value : '';
}

export const noProductName = {
  meta: {
    type: 'problem',
    docs: { description: "Forbid the product's name written in text." },
    schema: [
      {
        type: 'object',
        properties: { name: { type: 'string' } },
        required: ['name'],
        additionalProperties: false,
      },
    ],
    messages: {
      written: "Read the name from `PRODUCT_NAME` in '@wirecatch/product'.",
    },
  },
  create(context) {
    const word = new RegExp(String.raw`\b${context.options[0].name}\b`);
    const check = (node) => {
      if (word.test(textOf(node))) context.report({ node, messageId: 'written' });
    };
    return Object.fromEntries(TEXT_NODES.map((type) => [type, check]));
  },
};
