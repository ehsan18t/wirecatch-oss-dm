// The tester for rules that need type information: each test lints fixtures/file.ts with the code it gives, in the program of fixtures/tsconfig.json.
import { fileURLToPath } from 'node:url';
import { RuleTester } from 'eslint';
import ts from 'typescript-eslint';

/** The file every typed test lints; the code a test gives stands in for its contents. */
export const filename = fileURLToPath(new URL('fixtures/file.ts', import.meta.url));

/**
 * A RuleTester whose parser types each test's code.
 *
 * typescript-eslint builds its programs once for a whole run when it sees `CI=true`, as every CI service sets, and then types every test against that one program rather than the code each test gives. Its own RuleTester turns that inference off; ESLint's does not, so it is turned off here.
 */
export function typedRuleTester() {
  return new RuleTester({
    languageOptions: {
      parser: ts.parser,
      parserOptions: {
        project: './tsconfig.json',
        tsconfigRootDir: fileURLToPath(new URL('fixtures/', import.meta.url)),
        disallowAutomaticSingleRunInference: true,
      },
    },
  });
}
