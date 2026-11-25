import assert from 'node:assert/strict';
import { test } from 'node:test';
import { commitProblems, prTitleProblems } from './commits.mjs';

const EM_DASH = String.fromCodePoint(0x2014);
const VALID_72 = 'fix(desktop): clear unchanged queue page on leave and confirm deletes ab';
const OVER_72 = 'fix(desktop): clear unchanged queue page on leave and confirm deletes abc';

test('squash-merged PR suffix is excluded from the 72-character limit when checking commits', () => {
  const squashed = 'fix(desktop): clear unchanged queue page on leave and confirm deletes (#559)';
  assert.deepEqual(commitProblems(squashed, { stripPrSuffix: true }), []);
  assert.deepEqual(commitProblems(squashed), ['the subject is 76 characters; the limit is 72']);
  assert.deepEqual(commitProblems(`${OVER_72} (#559)`, { stripPrSuffix: true }), [
    'the subject is 73 characters; the limit is 72',
  ]);
  assert.deepEqual(
    commitProblems('fix(desktop): clear queue page. (#559)', { stripPrSuffix: true }),
    ['the subject must not end with a period'],
  );
  assert.equal(commitProblems('fix(desktop): (#559)', { stripPrSuffix: true }).length, 1);
  assert.deepEqual(
    commitProblems('fix(desktop): double suffix check (#123) (#456)', { stripPrSuffix: true }),
    [],
  );
  assert.deepEqual(commitProblems(`${OVER_72} (#123) (#456)`, { stripPrSuffix: true }), [
    'the subject is 80 characters; the limit is 72',
  ]);
});

test('pull request titles follow subject rules without autosquash or PR suffix exemptions', () => {
  assert.deepEqual(prTitleProblems(VALID_72), []);
  assert.deepEqual(prTitleProblems(OVER_72), ['the subject is 73 characters; the limit is 72']);
  assert.deepEqual(prTitleProblems('fix(desktop): clear unchanged queue page (#559)'), []);
  assert.deepEqual(
    prTitleProblems('fix(desktop): clear unchanged queue page on leave and confirm deletes (#559)'),
    ['the subject is 76 characters; the limit is 72'],
  );
  assert.equal(prTitleProblems('fixup! fix(desktop): temporary').length, 1);
  assert.deepEqual(prTitleProblems(`fix(desktop): bad ${EM_DASH} dash`), [
    'the message contains an em dash',
  ]);
  assert.deepEqual(prTitleProblems('fix(desktop): Uppercase start'), [
    'the subject must start in lower case',
  ]);
  assert.deepEqual(prTitleProblems('fix(desktop): trailing period.'), [
    'the subject must not end with a period',
  ]);
});

test('commit messages allow autosquash, ignore git comments and scissors, and check bodies', () => {
  assert.deepEqual(commitProblems('fixup! fix(desktop): wip'), []);
  assert.deepEqual(
    commitProblems(
      'fix(desktop): valid subject\n# ignored comment\n# ------------------------ >8 ------------------------\nCo-Authored-By: someone\n',
    ),
    [],
  );
  assert.deepEqual(
    commitProblems(`fix(desktop): valid subject\n\nBody with ${EM_DASH} and\nCo-Authored-By: bot`),
    [
      'the message contains an em dash',
      'the message credits a tool or AI as an author; remove that line',
    ],
  );
  assert.deepEqual(
    commitProblems(`fix(desktop): valid subject\n\nBody with ${EM_DASH} and\nCo-Authored-By: bot`, {
      subjectOnly: true,
    }),
    [],
  );
});
