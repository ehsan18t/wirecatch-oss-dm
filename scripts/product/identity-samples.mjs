// Test data for the values a release must decide, shared by the product tests.

export function withUpdateConfig(text, settings) {
  return text.replace(/^\[updates\]\n.*?(?=^\[)/ms, () => `[updates]\n${settings}\n\n`);
}

/** Each value of config/product.toml a release decides, as a table, a key, a placeholder and a decided value. */
export const UNDECIDED_VALUES = [
  ['publisher', 'legal_name', 'TODO-PRODUCT-PUBLISHER-LEGAL-NAME', 'Example Ltd'],
  ['links', 'website', 'https://TODO-PRODUCT-WEBSITE.invalid/', 'https://example.com/'],
  [
    'contacts',
    'support_email',
    'TODO-PRODUCT-SUPPORT-EMAIL@placeholder.invalid',
    'help@example.com',
  ],
  [
    'contacts',
    'security_email',
    'TODO-PRODUCT-SECURITY-EMAIL@placeholder.invalid',
    'security@example.com',
  ],
];
