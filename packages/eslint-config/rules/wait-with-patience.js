// A `vi.waitFor` is given its time limit: without one it gives up after a second, which a busy machine overruns (docs/ENGINEERING.md, section 7).

export const waitWithPatience = {
  meta: {
    type: 'problem',
    docs: { description: 'Require a time limit on vi.waitFor.' },
    schema: [],
    messages: {
      bare: "Give vi.waitFor a time limit: `vi.waitFor(check, PATIENCE_MS)`, from '@wirecatch/ui/testing/jsdom'.",
    },
  },
  create(context) {
    return {
      CallExpression(node) {
        const { callee } = node;
        if (
          callee.type === 'MemberExpression' &&
          callee.object.type === 'Identifier' &&
          callee.object.name === 'vi' &&
          callee.property.type === 'Identifier' &&
          callee.property.name === 'waitFor' &&
          node.arguments.length < 2
        ) {
          context.report({ node, messageId: 'bare' });
        }
      },
    };
  },
};
